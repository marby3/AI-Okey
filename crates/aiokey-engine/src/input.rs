//! 打った文字列を、入力の単位（ユニット）に分解する。
//!
//! 混在入力では `ka` を「k の母音を a に指定」とも「k の後にア行の あ」とも読めるので、
//! 結果は 1 本の列ではなく、文字位置の間に張った辺のグラフ（入力ラティス）で表す。
//! 段1のラティスは、この辺をそのままたどって辞書を引く。

/// Shift+Space で入れる空白。入力列ではこの文字で受け取り、区切りとして扱う。
/// 変換中の Space は変換に使うので入力列には入らず、入力列に現れる空白は Shift+Space だけになる。
pub const SEPARATOR: char = ' ';

/// 英語指定の始まり（SKK の略語入力と同じ考え方）。
pub const ENGLISH_PREFIX: char = '/';

/// 入力の単位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unit {
    /// 子音 1 文字（k s t n h m y r w g z d b p）。`vowel` は混在入力で指定された母音。
    Consonant { letter: char, vowel: Option<char> },
    /// ア行のかな（a i u e o）
    Vowel(char),
    /// ん（q）
    Hatsuon,
    /// っ（x）
    Sokuon,
    /// 長音（-）
    Long,
    /// `/` で始めた英単語（つづりのまま）
    English(String),
    /// Shift+Space の区切り
    Separator,
    /// そのほかの文字（数字・記号・想定外の文字）
    Other(char),
}

/// 入力ラティスの辺。`start..end` は入力の文字位置（char 単位）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub start: usize,
    pub end: usize,
    pub unit: Unit,
}

/// 入力ラティス。位置 0 から `len` までをつなぐ辺の集まり。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputLattice {
    len: usize,
    edges: Vec<Edge>,
}

const CONSONANTS: &str = "kstnhmyrwgzdbp";
const VOWELS: &str = "aiueo";

/// 打った文字列を入力ラティスにする。
pub fn parse(input: &str) -> InputLattice {
    let chars: Vec<char> = input.chars().collect();
    let mut edges = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let lower = c.to_ascii_lowercase();

        if c == SEPARATOR {
            edges.push(Edge {
                start: i,
                end: i + 1,
                unit: Unit::Separator,
            });
            i += 1;
            continue;
        }

        if c == ENGLISH_PREFIX {
            // 次の区切り（または末尾）までを英単語として扱う
            let end = chars[i + 1..]
                .iter()
                .position(|&c| c == SEPARATOR)
                .map_or(chars.len(), |p| i + 1 + p);
            let word: String = chars[i + 1..end].iter().collect();
            let unit = if word.is_empty() {
                Unit::Other(c)
            } else {
                Unit::English(word)
            };
            edges.push(Edge {
                start: i,
                end,
                unit,
            });
            i = end;
            continue;
        }

        let unit = match lower {
            'q' => Unit::Hatsuon,
            'x' => Unit::Sokuon,
            '-' => Unit::Long,
            _ if CONSONANTS.contains(lower) => {
                // 直後が母音なら、母音を指定した子音としての解釈も足す
                if let Some(&next) = chars.get(i + 1) {
                    let next = next.to_ascii_lowercase();
                    if VOWELS.contains(next) {
                        edges.push(Edge {
                            start: i,
                            end: i + 2,
                            unit: Unit::Consonant {
                                letter: lower,
                                vowel: Some(next),
                            },
                        });
                    }
                }
                Unit::Consonant {
                    letter: lower,
                    vowel: None,
                }
            }
            _ if VOWELS.contains(lower) => Unit::Vowel(lower),
            _ => Unit::Other(c),
        };
        edges.push(Edge {
            start: i,
            end: i + 1,
            unit,
        });
        i += 1;
    }
    InputLattice {
        len: chars.len(),
        edges,
    }
}

impl InputLattice {
    /// 入力の長さ（文字数）。
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// すべての辺。
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// 位置 `start` から出る辺。
    pub fn edges_from(&self, start: usize) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(move |e| e.start == start)
    }

    /// 先頭から末尾までのすべての読み方。数は入力に対して指数的に増えるので、
    /// テストや短い入力の確認用に使う。
    pub fn paths(&self) -> Vec<Vec<Unit>> {
        let mut out = Vec::new();
        let mut current = Vec::new();
        self.collect_paths(0, &mut current, &mut out);
        out
    }

    fn collect_paths(&self, pos: usize, current: &mut Vec<Unit>, out: &mut Vec<Vec<Unit>>) {
        if pos == self.len {
            out.push(current.clone());
            return;
        }
        for edge in self.edges_from(pos) {
            current.push(edge.unit.clone());
            self.collect_paths(edge.end, current, out);
            current.pop();
        }
    }
}
