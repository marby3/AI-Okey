//! 段2で、打った入力に合う文を生成する（#40 の試作）。
//!
//! 入力に合うかは、段1のラティスに置く語（表記と、入力のどこからどこまでを読むか）で決める。生成した文字列を
//! バイトごとに語の表記へ当てはめ、どれかの語の途中か、語の切れ目（入力の位置）にいる間だけ続けてよい。
//! 漢字の表記も、段1の辞書でその読みが入力と合うときだけ許される。終わり（EOS）は、入力の末尾の切れ目に
//! いるときだけ許す。この制約の上で、段2のモデルの log P の和に、段1の語のコストを重みつきで足した値で
//! ビームサーチする（学習済みの段2は候補を並べ替える学習しかしておらず、log P だけでは辞書にないかなを
//! 並べた文が上に来るため）。

use std::collections::{HashMap, HashSet};

use llama_cpp_2::token::LlamaToken;

/// 制約に使う語。`start..end` は入力の文字位置。
pub struct Word<'a> {
    pub start: usize,
    pub end: usize,
    pub surface: &'a str,
    /// 段1の語のコスト
    pub cost: i32,
}

/// 段1のコストを log P（自然対数）に直すときの割る数（Mozc のコストはおよそ −500 × log P）。
const COST_PER_NAT: f32 = 500.0;

/// 生成した文字列が、いまどこにいるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Pos {
    /// 語の切れ目。入力の位置
    At(usize),
    /// 語（番号）の表記の途中。何バイト目まで当てはめたか
    In(u32, u16),
}

/// 状態の 1 つ：位置と、そこまでの段1のコストの和（入りかけの語のコストも含む）。
pub(crate) type Item = (Pos, i32);

/// 入力に合う文字列を受け付けるオートマトン。状態は [`Item`] の集まり（位置ごとにコストの最小を残す）。
pub struct Constraint {
    len: usize,
    surfaces: Vec<Vec<u8>>,
    ends: Vec<usize>,
    costs: Vec<i32>,
    /// (開始位置, 表記の先頭のバイト) → 語の番号
    first: HashMap<(usize, u8), Vec<u32>>,
}

impl Constraint {
    pub fn new(len: usize, words: &[Word]) -> Self {
        let mut first: HashMap<(usize, u8), Vec<u32>> = HashMap::new();
        let mut surfaces = Vec::with_capacity(words.len());
        let mut ends = Vec::with_capacity(words.len());
        let mut costs = Vec::with_capacity(words.len());
        for w in words {
            let bytes = w.surface.as_bytes().to_vec();
            if bytes.is_empty() || bytes.len() > u16::MAX as usize {
                continue;
            }
            first
                .entry((w.start, bytes[0]))
                .or_default()
                .push(surfaces.len() as u32);
            surfaces.push(bytes);
            ends.push(w.end);
            costs.push(w.cost);
        }
        for ids in first.values_mut() {
            // 同じ区間・同じ表記の語（品詞違い）は、コストのいちばん低いもの 1 つでよい
            ids.sort_unstable_by(|&a, &b| {
                let key = |i: u32| (&surfaces[i as usize], ends[i as usize], costs[i as usize]);
                key(a).cmp(&key(b))
            });
            ids.dedup_by(|a, b| {
                surfaces[*a as usize] == surfaces[*b as usize]
                    && ends[*a as usize] == ends[*b as usize]
            });
        }
        Self {
            len,
            surfaces,
            ends,
            costs,
            first,
        }
    }

    pub(crate) fn start(&self) -> Vec<Item> {
        vec![(Pos::At(0), 0)]
    }

    fn advance(&self, word: u32, offset: usize, cost: i32, out: &mut Vec<Item>) {
        if offset == self.surfaces[word as usize].len() {
            out.push((Pos::At(self.ends[word as usize]), cost));
        } else {
            out.push((Pos::In(word, offset as u16), cost));
        }
    }

    /// 1 バイト進めた状態。空なら、そのバイトは入力に合わない。
    fn step(&self, state: &[Item], byte: u8) -> Vec<Item> {
        let mut out = Vec::new();
        for &(p, cost) in state {
            match p {
                Pos::At(i) => {
                    for &w in self.first.get(&(i, byte)).into_iter().flatten() {
                        self.advance(w, 1, cost + self.costs[w as usize], &mut out);
                    }
                }
                Pos::In(w, k) => {
                    if self.surfaces[w as usize][k as usize] == byte {
                        self.advance(w, k as usize + 1, cost, &mut out);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup_by_key(|item| item.0);
        out
    }

    /// 入力の末尾の切れ目にいれば、そこまでのコスト。
    fn end_cost(&self, state: &[Item]) -> Option<i32> {
        state
            .iter()
            .find(|(p, _)| *p == Pos::At(self.len))
            .map(|&(_, c)| c)
    }
}

/// いちばん低いコストを log P の単位に直した罰（重み `weight` をかける）。
fn penalty(state: &[Item], weight: f32) -> f32 {
    let cost = state.iter().map(|&(_, c)| c).min().unwrap_or(0);
    weight * cost as f32 / COST_PER_NAT
}

/// 語彙のトークンの表記（バイト列）のトライ。制約に合うトークンだけを、枝を刈りながら列挙する。
pub(crate) struct VocabTrie {
    /// ノードごとの (バイト, 子ノード)
    children: Vec<Vec<(u8, usize)>>,
    /// ノードで終わるトークン
    tokens: Vec<Vec<LlamaToken>>,
}

impl VocabTrie {
    pub(crate) fn new(pieces: impl Iterator<Item = (LlamaToken, Vec<u8>)>) -> Self {
        let mut trie = Self {
            children: vec![Vec::new()],
            tokens: vec![Vec::new()],
        };
        for (token, bytes) in pieces {
            if bytes.is_empty() {
                continue;
            }
            let mut node = 0;
            for b in bytes {
                node = match trie.children[node].iter().find(|(c, _)| *c == b) {
                    Some(&(_, child)) => child,
                    None => {
                        trie.children.push(Vec::new());
                        trie.tokens.push(Vec::new());
                        let child = trie.children.len() - 1;
                        trie.children[node].push((b, child));
                        child
                    }
                };
            }
            trie.tokens[node].push(token);
        }
        trie
    }

    /// 状態 `state` から出せるトークンと、その表記、出したあとの状態。
    fn allowed(
        &self,
        constraint: &Constraint,
        state: &[Item],
    ) -> Vec<(LlamaToken, Vec<u8>, Vec<Item>)> {
        let mut out = Vec::new();
        let mut stack = vec![(0usize, Vec::new(), state.to_vec())];
        while let Some((node, piece, state)) = stack.pop() {
            for &(byte, child) in &self.children[node] {
                let next = constraint.step(&state, byte);
                if next.is_empty() {
                    continue;
                }
                let mut piece = piece.clone();
                piece.push(byte);
                for &t in &self.tokens[child] {
                    out.push((t, piece.clone(), next.clone()));
                }
                stack.push((child, piece, next));
            }
        }
        out
    }
}

/// 生成の設定。
#[derive(Debug, Clone)]
pub struct GenerateOptions {
    /// ビームの幅（出す候補の数の上限も兼ねる）
    pub beam: usize,
    /// 段1のコストの重み（0 なら段2の log P だけ）
    pub cost_weight: f32,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            beam: 8,
            cost_weight: 0.5,
        }
    }
}

/// ビームの 1 本。
pub(crate) struct Beam {
    pub seq: i32,
    pub tokens: Vec<LlamaToken>,
    pub bytes: Vec<u8>,
    pub state: Vec<Item>,
    /// 段2の log P の和
    pub log_p: f32,
    /// 並べる値（log P − 段1のコストの罰）
    pub score: f32,
    /// 次のトークンの log P
    pub row: Vec<f32>,
}

/// ビームを 1 段伸ばす候補。
pub(crate) struct Expansion {
    pub parent: usize,
    pub token: LlamaToken,
    /// 伸ばしたあとの表記
    pub bytes: Vec<u8>,
    pub state: Vec<Item>,
    pub log_p: f32,
    pub score: f32,
}

/// 生成の 1 段：各ビームから出せるトークンを集め、終わったもの（EOS）と、伸ばす上位 `beam` 本に分ける。
/// 同じ表記になる伸ばし方（トークンの切り方の違い）は、並べる値の高いほうだけを残してから絞る。
pub(crate) fn expand(
    beams: &[Beam],
    trie: &VocabTrie,
    constraint: &Constraint,
    eos: LlamaToken,
    beam: usize,
    weight: f32,
    finished: &mut Vec<(Vec<u8>, f32)>,
) -> Vec<Expansion> {
    let mut all = Vec::new();
    for (i, b) in beams.iter().enumerate() {
        if let Some(cost) = constraint.end_cost(&b.state) {
            let log_p = b.log_p + b.row[eos.0 as usize];
            finished.push((b.bytes.clone(), log_p - weight * cost as f32 / COST_PER_NAT));
        }
        for (token, piece, state) in trie.allowed(constraint, &b.state) {
            let log_p = b.log_p + b.row[token.0 as usize];
            let mut bytes = b.bytes.clone();
            bytes.extend(piece);
            all.push(Expansion {
                parent: i,
                token,
                bytes,
                score: log_p - penalty(&state, weight),
                state,
                log_p,
            });
        }
    }
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen = HashSet::new();
    all.retain(|x| seen.insert(x.bytes.clone()));
    all.truncate(beam);
    all
}

/// 終わった候補を、表記の重複を除いて並べる値の高い順に `n` 件まで並べる。
pub(crate) fn finish(mut finished: Vec<(Vec<u8>, f32)>, n: usize) -> Vec<(String, f32)> {
    finished.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<(String, f32)> = Vec::new();
    for (bytes, score) in finished {
        let Ok(s) = String::from_utf8(bytes) else {
            continue;
        };
        if !out.iter().any(|(t, _)| *t == s) {
            out.push((s, score));
        }
        if out.len() >= n {
            break;
        }
    }
    out
}

/// まだ伸ばす価値があるか（log P は伸ばすほど下がり、コストは増えるので、残りのビームが n 番目の終わった候補より
/// 低ければ止める）。
pub(crate) fn worth_continuing(beams: &[Beam], finished: &[(Vec<u8>, f32)], n: usize) -> bool {
    if beams.is_empty() {
        return false;
    }
    let mut scores: Vec<f32> = finished.iter().map(|f| f.1).collect();
    if scores.len() < n {
        return true;
    }
    scores.sort_by(|a, b| b.total_cmp(a));
    let best = beams
        .iter()
        .map(|b| b.score)
        .fold(f32::NEG_INFINITY, f32::max);
    best > scores[n - 1]
}
