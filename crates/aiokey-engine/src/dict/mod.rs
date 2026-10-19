//! スケルトン索引辞書。スケルトンから単語を引く。

use std::borrow::Cow;
use std::path::Path;

/// 辞書の 1 語。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<'a> {
    pub surface: Cow<'a, str>,
    pub reading: &'a str,
    pub lid: u16,
    pub rid: u16,
    pub cost: i16,
}

/// 辞書ファイルを開けなかった理由。
#[derive(Debug)]
pub struct DictError(pub String);

/// スケルトン索引辞書。
pub struct Dictionary {}

impl Dictionary {
    pub fn open(_path: &Path) -> Result<Self, DictError> {
        unimplemented!()
    }

    pub fn from_bytes(_bytes: Vec<u8>) -> Result<Self, DictError> {
        unimplemented!()
    }

    pub fn lookup(&self, _skeleton: &str) -> Vec<Entry<'_>> {
        unimplemented!()
    }

    pub fn common_prefix_search(&self, _input: &str) -> Vec<(usize, Vec<Entry<'_>>)> {
        unimplemented!()
    }

    pub fn cost(&self, _rid: u16, _lid: u16) -> i16 {
        unimplemented!()
    }

    pub fn pos_count(&self) -> u16 {
        unimplemented!()
    }

    pub fn is_mmapped(&self) -> bool {
        unimplemented!()
    }

    pub fn size_bytes(&self) -> usize {
        unimplemented!()
    }
}
