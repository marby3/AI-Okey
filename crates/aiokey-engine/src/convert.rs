//! 段1の変換：入力からラティスを組み、上位 K 件の候補を出す。
//!
//! 1. 入力ラティス（[`crate::input`]）を各開始位置からたどり、たどったスケルトンで辞書を共通接頭辞検索する。
//!    混在入力で打った母音は、読みの各かなの母音と照らし合わせて絞る。
//! 2. 未知語に備えて、入力の 1 単位ごとにひらがな（子音なら各段のかな）の語を高いコストで足す。
//! 3. 前向きの Viterbi で各語までの最小コストを求め、後ろ向きの A* で表記の重複しない上位 K 件を出す。

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use crate::dict::Dictionary;
use crate::input::{parse, InputLattice, Unit};
use crate::skeleton;

/// 1 語がたどる入力の単位の上限。
const MAX_UNITS: usize = 16;
/// 1 つの開始位置からたどる道の上限（母音を多く打つと道が増える）。
const MAX_PATHS_PER_START: usize = 128;
/// 同じ区間（開始位置と終了位置）・同じ読み方（キーと打った母音）に置く辞書の語の上限。
/// コストの低い順に残す。読み方ごとに数えるのは、`kik` の「き＋子音」と「子音＋い＋子音」のように、
/// 解釈の違う語どうしで枠を取り合わないようにするため。
const MAX_ENTRIES_PER_SPAN: usize = 40;
/// 未知語（辞書にないかな 1 文字）のコスト。
const UNKNOWN_COST: i32 = 9000;
/// A* で取り出す回数の上限。
const MAX_POPS: usize = 50_000;
/// A* で 1 回に広げる前の語の上限。
const MAX_EXPAND: usize = 64;

/// 子音ごとのかな（未知語の候補に使う）。
const ROWS: &[(char, &str)] = &[
    ('k', "かきくけこ"),
    ('s', "さしすせそ"),
    ('t', "たちつてと"),
    ('n', "なにぬねの"),
    ('h', "はひふへほ"),
    ('m', "まみむめも"),
    ('y', "やゆよ"),
    ('r', "らりるれろ"),
    ('w', "わを"),
    ('g', "がぎぐげご"),
    ('z', "ざじずぜぞ"),
    ('d', "だぢづでど"),
    ('b', "ばびぶべぼ"),
    ('p', "ぱぴぷぺぽ"),
];
const VOWEL_KANA: &[(char, char)] = &[
    ('a', 'あ'),
    ('i', 'い'),
    ('u', 'う'),
    ('e', 'え'),
    ('o', 'お'),
];

/// 変換の設定。
#[derive(Debug, Clone)]
pub struct Options {
    /// 出す候補の数の上限
    pub k: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { k: 32 }
    }
}

/// 候補の中の 1 語。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub surface: String,
    pub reading: String,
}

/// 候補文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub surface: String,
    pub reading: String,
    pub cost: i32,
    pub words: Vec<Word>,
    /// 文節ごとの入力の範囲（文字位置）
    pub bunsetsu: Vec<Range<usize>>,
}

/// ラティスの語。
#[derive(Debug, Clone)]
struct Node {
    start: usize,
    end: usize,
    surface: String,
    reading: String,
    lid: u16,
    rid: u16,
    cost: i32,
}

/// 入力を変換し、コストの低い順に最大 `options.k` 件の候補を返す。
pub fn convert(dict: &Dictionary, input: &str, options: &Options) -> Vec<Candidate> {
    let lattice = parse(input);
    if lattice.is_empty() || options.k == 0 {
        return Vec::new();
    }
    let nodes = build_nodes(dict, &lattice);
    let mut candidates = search(dict, &nodes, lattice.len(), options.k);
    add_raw_input(input, lattice.len(), &mut candidates, options.k);
    candidates
}

/// 英語指定の印の `/` を取り除く。入力パーサーと同じく、英単語の外にある `/` だけが印で、
/// 英単語の中（次の区切りまで）の `/` はそのまま残す。
fn without_english_prefixes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_english = false;
    for c in input.chars() {
        if c == crate::input::ENGLISH_PREFIX && !in_english {
            in_english = true;
            continue;
        }
        if c == crate::input::SEPARATOR {
            in_english = false;
        }
        out.push(c);
    }
    out
}

/// 入力した文字列そのもの（英語指定の印の `/` は除く）を、必ず候補に入れる。
///
/// 候補がすでに K 件あれば最後の 1 件と入れ替える。並びが崩れないよう、コストは
/// ほかの候補のいちばん大きいコスト以上にする。ただし K = 1 で候補があるときは、
/// いちばんよい候補を優先して入れない（#30）。
fn add_raw_input(input: &str, len: usize, candidates: &mut Vec<Candidate>, k: usize) {
    let raw = without_english_prefixes(input);
    if k == 0 || raw.is_empty() || candidates.iter().any(|c| c.surface == raw) {
        return;
    }
    if k == 1 && !candidates.is_empty() {
        return;
    }
    if candidates.len() >= k {
        candidates.truncate(k.saturating_sub(1));
    }
    let cost = candidates.iter().map(|c| c.cost).max().unwrap_or(0);
    candidates.push(Candidate {
        surface: raw.clone(),
        reading: raw.clone(),
        cost,
        words: vec![Word {
            surface: raw.clone(),
            reading: raw,
        }],
        bunsetsu: std::iter::once(0..len).collect(),
    });
}

/// 入力ラティスの 1 本の道（1 語ぶん）。
#[derive(Clone, Default)]
struct Path {
    key: Vec<u8>,
    ends: Vec<usize>,
    vowels: Vec<Option<char>>,
}

/// スケルトンとして辞書を引ける単位なら、そのスケルトンの 1 文字と、指定された母音を返す。
fn skeletal(unit: &Unit) -> Option<(u8, Option<char>)> {
    match unit {
        Unit::Consonant { letter, vowel } => Some((*letter as u8, *vowel)),
        Unit::Vowel(v) => Some((*v as u8, None)),
        Unit::Hatsuon => Some((b'q', None)),
        Unit::Sokuon => Some((b'x', None)),
        Unit::Long => Some((b'-', None)),
        _ => None,
    }
}

/// `start` からたどれる道を、それ以上伸ばせないところまで集める。
fn paths_from(lattice: &InputLattice, start: usize) -> Vec<Path> {
    let mut leaves = Vec::new();
    let mut stack = vec![(start, Path::default())];
    while let Some((pos, path)) = stack.pop() {
        let mut extended = false;
        if path.key.len() < MAX_UNITS && leaves.len() + stack.len() < MAX_PATHS_PER_START {
            for edge in lattice.edges_from(pos) {
                if let Some((byte, vowel)) = skeletal(&edge.unit) {
                    let mut next = path.clone();
                    next.key.push(byte);
                    next.ends.push(edge.end);
                    next.vowels.push(vowel);
                    stack.push((edge.end, next));
                    extended = true;
                }
            }
        }
        if !extended && !path.key.is_empty() {
            leaves.push(path);
        }
    }
    leaves
}

/// 読みが、打った母音と矛盾しないか。
fn vowels_match(reading: &str, vowels: &[Option<char>]) -> bool {
    let units = skeleton::units(reading);
    units.len() == vowels.len()
        && units
            .iter()
            .zip(vowels)
            .all(|(&(_, have), want)| want.is_none() || have == *want)
}

fn build_nodes(dict: &Dictionary, lattice: &InputLattice) -> Vec<Node> {
    let mut nodes = Vec::new();
    for start in 0..lattice.len() {
        // 読み方（キーと打った母音）ごとに、終了位置と、合う語の (コスト, 番号)
        let mut groups: Vec<(usize, Vec<(i16, usize)>)> = Vec::new();
        let mut seen_entries: HashSet<(usize, usize)> = HashSet::new();
        let mut seen_prefixes: HashSet<(Vec<u8>, Vec<Option<char>>)> = HashSet::new();
        for path in paths_from(lattice, start) {
            for (len, range) in dict.prefix_groups(&path.key) {
                let vowels = &path.vowels[..len];
                if !seen_prefixes.insert((path.key[..len].to_vec(), vowels.to_vec())) {
                    continue;
                }
                let end = path.ends[len - 1];
                let constrained = vowels.iter().any(Option::is_some);
                let mut found = Vec::new();
                for index in range {
                    // 打った母音と合う語だけを、処理済みとして記録する
                    if seen_entries.contains(&(end, index))
                        || (constrained && !vowels_match(&dict.entry_at(index).reading, vowels))
                    {
                        continue;
                    }
                    seen_entries.insert((end, index));
                    let (cost, _, _) = dict.entry_cost_and_pos(index);
                    found.push((cost, index));
                }
                groups.push((end, found));
            }
        }
        // 同じ区間に同じ表記・品詞の語を 2 つ置かない
        let mut surfaces: HashSet<(usize, String, u16, u16)> = HashSet::new();
        for (end, mut found) in groups {
            found.sort_unstable();
            let mut placed = 0;
            for (_, index) in found {
                let e = dict.entry_at(index);
                if !surfaces.insert((end, e.surface.to_string(), e.lid, e.rid)) {
                    continue;
                }
                nodes.push(Node {
                    start,
                    end,
                    surface: e.surface.into_owned(),
                    reading: e.reading,
                    lid: e.lid,
                    rid: e.rid,
                    cost: e.cost as i32,
                });
                placed += 1;
                if placed >= MAX_ENTRIES_PER_SPAN {
                    break;
                }
            }
        }
    }
    add_unknown_nodes(dict, lattice, &mut nodes);
    nodes
}

/// 入力の 1 単位ごとに、辞書にない語として置くかなを足す。
fn add_unknown_nodes(dict: &Dictionary, lattice: &InputLattice, nodes: &mut Vec<Node>) {
    let pos = dict.unknown_pos();
    for edge in lattice.edges() {
        let mut push = |text: String, cost: i32| {
            nodes.push(Node {
                start: edge.start,
                end: edge.end,
                surface: text.clone(),
                reading: text,
                lid: pos,
                rid: pos,
                cost,
            })
        };
        match &edge.unit {
            Unit::Consonant { letter, vowel } => {
                let row = ROWS.iter().find(|(l, _)| l == letter).map(|&(_, r)| r);
                for kana in row.into_iter().flat_map(str::chars) {
                    if vowel.is_none() || skeleton::units(&kana.to_string())[0].1 == *vowel {
                        push(kana.to_string(), UNKNOWN_COST);
                    }
                }
            }
            Unit::Vowel(v) => {
                if let Some(&(_, kana)) = VOWEL_KANA.iter().find(|(x, _)| x == v) {
                    push(kana.to_string(), UNKNOWN_COST);
                }
            }
            Unit::Hatsuon => push("ん".to_string(), UNKNOWN_COST),
            Unit::Sokuon => push("っ".to_string(), UNKNOWN_COST),
            Unit::Long => push("ー".to_string(), UNKNOWN_COST),
            Unit::Separator => push(" ".to_string(), 0),
            Unit::English(word) => push(word.clone(), UNKNOWN_COST),
            Unit::Other(c) => push(c.to_string(), UNKNOWN_COST),
        }
    }
}

/// BOS（文頭）と EOS（文末）の文脈 ID。
const BOS_EOS: u16 = 0;

/// A* でたどった道（EOS 側から BOS 側へ伸ばす）。
struct Trail {
    node: usize,
    next: Option<Rc<Trail>>,
}

/// A* のヒープに入れる状態。優先度（小さいほど先）と、入れた順だけで比べる。
struct Item {
    priority: i32,
    seq: usize,
    /// 次に広げる語。`None` なら BOS に着いた
    node: Option<usize>,
    /// この語より右側の確定したコスト（この語のコストは含まない）
    after: i32,
    trail: Option<Rc<Trail>>,
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        (self.priority, self.seq) == (other.priority, other.seq)
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Item {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (Reverse(self.priority), Reverse(self.seq))
            .cmp(&(Reverse(other.priority), Reverse(other.seq)))
    }
}

fn search(dict: &Dictionary, nodes: &[Node], len: usize, k: usize) -> Vec<Candidate> {
    let mut starts_at: Vec<Vec<usize>> = vec![Vec::new(); len + 1];
    let mut ends_at: Vec<Vec<usize>> = vec![Vec::new(); len + 1];
    for (i, node) in nodes.iter().enumerate() {
        starts_at[node.start].push(i);
        ends_at[node.end].push(i);
    }

    // 前向き：forward[i] は BOS から語 i まで（語 i のコストを含む）の最小コスト
    let mut forward = vec![i32::MAX; nodes.len()];
    for (pos, starting) in starts_at.iter().enumerate().take(len) {
        let best_by_rid = best_by_rid(pos, &ends_at, &forward, nodes);
        if best_by_rid.is_empty() {
            continue;
        }
        let mut by_lid: HashMap<u16, i32> = HashMap::new();
        for &i in starting {
            let node = &nodes[i];
            let into = *by_lid
                .entry(node.lid)
                .or_insert_with(|| min_into(dict, &best_by_rid, node.lid));
            forward[i] = into.saturating_add(node.cost);
        }
    }
    if best_by_rid(len, &ends_at, &forward, nodes).is_empty() {
        return Vec::new();
    }

    // 後ろ向き：priority = forward（左側の最小） + after（右側の確定したコスト）
    let mut heap: BinaryHeap<Item> = BinaryHeap::new();
    let mut seq = 0;
    let mut push = |heap: &mut BinaryHeap<Item>, priority, node, after, trail| {
        heap.push(Item {
            priority,
            seq,
            node,
            after,
            trail,
        });
        seq += 1;
    };
    for &i in &ends_at[len] {
        if forward[i] == i32::MAX {
            continue;
        }
        let after = dict.cost(nodes[i].rid, BOS_EOS) as i32;
        push(
            &mut heap,
            forward[i].saturating_add(after),
            Some(i),
            after,
            None,
        );
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut pops = 0;
    while let Some(Item {
        priority: total,
        node,
        after,
        trail,
        ..
    }) = heap.pop()
    {
        pops += 1;
        if pops > MAX_POPS {
            break;
        }
        let Some(i) = node else {
            // BOS に着いた：1 本の候補ができた
            let candidate = to_candidate(dict, nodes, trail, total);
            if seen.insert(candidate.surface.clone()) {
                out.push(candidate);
                if out.len() >= k {
                    break;
                }
            }
            continue;
        };
        let here = &nodes[i];
        let trail = Some(Rc::new(Trail {
            node: i,
            next: trail,
        }));
        let after = after + here.cost;
        if here.start == 0 {
            let total = after + dict.cost(BOS_EOS, here.lid) as i32;
            push(&mut heap, total, None, total, trail);
            continue;
        }
        let mut prevs: Vec<(i32, usize, i32)> = ends_at[here.start]
            .iter()
            .filter(|&&p| forward[p] != i32::MAX)
            .map(|&p| {
                let a = after + dict.cost(nodes[p].rid, here.lid) as i32;
                (forward[p].saturating_add(a), p, a)
            })
            .collect();
        prevs.sort_unstable();
        for (priority, p, a) in prevs.into_iter().take(MAX_EXPAND) {
            push(&mut heap, priority, Some(p), a, trail.clone());
        }
    }
    out
}

/// 位置 `pos` で終わる語の、右文脈 ID ごとの最小の forward。`pos` が 0 なら BOS だけ。
fn best_by_rid(
    pos: usize,
    ends_at: &[Vec<usize>],
    forward: &[i32],
    nodes: &[Node],
) -> HashMap<u16, i32> {
    let mut best = HashMap::new();
    if pos == 0 {
        best.insert(BOS_EOS, 0);
        return best;
    }
    for &i in &ends_at[pos] {
        if forward[i] == i32::MAX {
            continue;
        }
        let e = best.entry(nodes[i].rid).or_insert(i32::MAX);
        *e = (*e).min(forward[i]);
    }
    best
}

fn min_into(dict: &Dictionary, best_by_rid: &HashMap<u16, i32>, lid: u16) -> i32 {
    best_by_rid
        .iter()
        .map(|(&rid, &f)| f.saturating_add(dict.cost(rid, lid) as i32))
        .min()
        .unwrap_or(i32::MAX)
}

fn to_candidate(
    dict: &Dictionary,
    nodes: &[Node],
    trail: Option<Rc<Trail>>,
    cost: i32,
) -> Candidate {
    let mut words = Vec::new();
    let mut bunsetsu: Vec<Range<usize>> = Vec::new();
    let mut current = trail;
    while let Some(t) = current {
        let node = &nodes[t.node];
        match bunsetsu.last_mut() {
            Some(last) if dict.is_function_word(node.lid) && node.lid != dict.unknown_pos() => {
                last.end = node.end;
            }
            _ => bunsetsu.push(node.start..node.end),
        }
        words.push(Word {
            surface: node.surface.clone(),
            reading: node.reading.clone(),
        });
        current = t.next.clone();
    }
    Candidate {
        surface: words.iter().map(|w| w.surface.as_str()).collect(),
        reading: words.iter().map(|w| w.reading.as_str()).collect(),
        cost,
        words,
        bunsetsu,
    }
}
