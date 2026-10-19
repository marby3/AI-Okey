//! 段1の変換：入力からラティスを組み、上位 K 件の候補を出す。

use std::ops::Range;

use crate::dict::Dictionary;

/// 変換の設定。
#[derive(Debug, Clone)]
pub struct Options {
    /// 出す候補の数の上限
    pub k: usize,
}

impl Default for Options {
    fn default() -> Self {
        unimplemented!()
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

/// 入力を変換し、コストの低い順に最大 `options.k` 件の候補を返す。
pub fn convert(_dict: &Dictionary, _input: &str, _options: &Options) -> Vec<Candidate> {
    unimplemented!()
}
