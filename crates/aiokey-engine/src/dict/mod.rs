//! スケルトン索引辞書。スケルトン（`htst` など）から単語（果たして など）を引く。
//!
//! 辞書は 1 つのファイルで、mmap で開いてそのまま使う。形式は [`format`] を参照。
//! 作るのは [`build`]（Mozc の辞書を読むのは `aiokey-dictbuild`）。

mod build;
mod format;
mod kana;

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::path::Path;

pub use build::{build, build_with_report, BuildReport, PosTable, SourceEntry};
use format::{
    item, read_u16, read_u32, read_u64, words_per_row, Header, Section, ENTRY_SIZE, FLAG_KATAKANA,
    FLAG_MASK, FLAG_SAME_AS_READING, POS_FUNCTION_WORD, SECTION_NAMES,
};

/// 辞書の 1 語。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<'a> {
    /// 表記（果たして）
    pub surface: Cow<'a, str>,
    /// 読み（はたして）
    pub reading: String,
    /// 左文脈 ID（品詞）
    pub lid: u16,
    /// 右文脈 ID（品詞）
    pub rid: u16,
    /// 単語コスト（小さいほど出やすい）
    pub cost: i16,
}

/// 辞書ファイルを開けなかった理由。
#[derive(Debug)]
pub struct DictError(pub String);

impl fmt::Display for DictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid dictionary: {}", self.0)
    }
}

impl std::error::Error for DictError {}

enum Data {
    Mmap(memmap2::Mmap),
    Owned(Vec<u8>),
}

impl Deref for Data {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Data::Mmap(m) => m,
            Data::Owned(v) => v,
        }
    }
}

/// スケルトン索引辞書。
pub struct Dictionary {
    data: Data,
    header: Header,
}

impl Dictionary {
    /// 辞書ファイルを mmap で開く。中身を確かめてから使う（壊れていればエラー）。
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let file =
            std::fs::File::open(path).map_err(|e| DictError(format!("{}: {e}", path.display())))?;
        // SAFETY: 開いたあとで外からファイルを書き換えられると、読む中身が変わる。
        // 辞書はインストール先に置く読み取り専用のファイルとして扱う。
        let mmap = unsafe { memmap2::Mmap::map(&file) }
            .map_err(|e| DictError(format!("{}: {e}", path.display())))?;
        Self::new(Data::Mmap(mmap))
    }

    /// メモリ上のバイト列から開く（テストや、ファイルを使わない環境向け）。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, DictError> {
        Self::new(Data::Owned(bytes))
    }

    fn new(data: Data) -> Result<Self, DictError> {
        let header = format::validate(&data)?;
        Ok(Self { data, header })
    }

    /// スケルトンが完全に一致する語。
    pub fn lookup(&self, skeleton: &str) -> Vec<Entry<'_>> {
        let key = skeleton.as_bytes();
        if key.is_empty() || key.contains(&0) {
            return Vec::new();
        }
        self.trie()
            .exact_match_search(key)
            .map(|group| self.group(group))
            .unwrap_or_default()
    }

    /// 入力の先頭と一致するスケルトンの語を、一致した長さ（バイト数）ごとに返す。
    pub fn common_prefix_search(&self, input: &str) -> Vec<(usize, Vec<Entry<'_>>)> {
        // トライのキーに NUL は入らないので、NUL の手前までを探す
        let bytes = input.as_bytes();
        let key = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
        let trie = self.trie();
        trie.common_prefix_search(key)
            .map(|(group, len)| (len, self.group(group)))
            .filter(|(_, entries)| !entries.is_empty())
            .collect()
    }

    /// 連接コスト。左の語の右文脈 ID（`rid`）と、右の語の左文脈 ID（`lid`）で引く。
    /// 範囲外の ID は、つながらないものとして最大のコストを返す。
    pub fn cost(&self, rid: u16, lid: u16) -> i16 {
        let n = self.header.pos_count;
        if rid >= n || lid >= n {
            return i16::MAX;
        }
        let (rid, lid) = (rid as usize, lid as usize);
        let word = rid * words_per_row(n) + lid / 64;
        let bits = read_u64(self.section(Section::ConnBits), word);
        let bit = 1u64 << (lid % 64);
        if bits & bit == 0 {
            return read_u16(self.section(Section::ConnModes), rid) as i16;
        }
        let rank =
            read_u32(self.section(Section::ConnRanks), word) + (bits & (bit - 1)).count_ones();
        read_u16(self.section(Section::ConnValues), rank as usize) as i16
    }

    /// スケルトンの先頭と一致する語のグループを、一致した長さ（バイト数）と語の番号の範囲で返す。
    /// 語の中身は [`Dictionary::entry_at`] などで、必要なものだけ取り出す。
    pub fn prefix_groups(&self, key: &[u8]) -> Vec<(usize, std::ops::Range<usize>)> {
        let key = &key[..key.iter().position(|&b| b == 0).unwrap_or(key.len())];
        let groups = self.section(Section::Groups);
        let count = groups.len() / 4;
        self.trie()
            .common_prefix_search(key)
            .filter(|&(g, _)| (g as usize) + 1 < count)
            .map(|(g, len)| {
                let g = g as usize;
                let range = read_u32(groups, g) as usize..read_u32(groups, g + 1) as usize;
                (len, range)
            })
            .collect()
    }

    /// 番号 `index` の語のコストと品詞（lid, rid）。文字列は作らない。
    pub fn entry_cost_and_pos(&self, index: usize) -> (i16, u16, u16) {
        let e = &self.section(Section::Entries)[index * ENTRY_SIZE..(index + 1) * ENTRY_SIZE];
        let pair = u16::from_le_bytes([e[8], e[9]]) as usize;
        let pairs = self.section(Section::PosPairs);
        (
            i16::from_le_bytes([e[10], e[11]]),
            read_u16(pairs, pair * 2),
            read_u16(pairs, pair * 2 + 1),
        )
    }

    /// 番号 `index` の語。
    pub fn entry_at(&self, index: usize) -> Entry<'_> {
        self.entry(index)
    }

    /// 未知語に使う品詞。
    pub fn unknown_pos(&self) -> u16 {
        self.header.unknown_pos
    }

    /// 付属語（助詞・助動詞・接尾辞など）の品詞か。範囲外の ID は付属語でないとみなす。
    pub fn is_function_word(&self, pos: u16) -> bool {
        self.section(Section::PosFlags)
            .get(pos as usize)
            .is_some_and(|f| f & POS_FUNCTION_WORD != 0)
    }

    /// 品詞（文脈 ID）の数。
    pub fn pos_count(&self) -> u16 {
        self.header.pos_count
    }

    /// 部分ごとの大きさ（バイト）。辞書の大きさを詰めるときの目安に使う。
    pub fn section_sizes(&self) -> Vec<(&'static str, usize)> {
        SECTION_NAMES
            .into_iter()
            .zip(self.header.sections.iter().map(|&(_, len)| len as usize))
            .collect()
    }

    /// mmap で開いているか。
    pub fn is_mmapped(&self) -> bool {
        matches!(self.data, Data::Mmap(_))
    }

    /// 辞書のデータの大きさ（バイト）。ファイルから開いたならファイルの大きさ。
    pub fn size_bytes(&self) -> usize {
        self.data.len()
    }

    fn section(&self, section: Section) -> &[u8] {
        let (offset, len) = self.header.sections[section as usize];
        &self.data[offset as usize..(offset + len) as usize]
    }

    fn trie(&self) -> yada::DoubleArray<&[u8]> {
        yada::DoubleArray::new(self.section(Section::Trie))
    }

    fn group(&self, group: u32) -> Vec<Entry<'_>> {
        let groups = self.section(Section::Groups);
        let group = group as usize;
        if group + 1 >= groups.len() / 4 {
            return Vec::new();
        }
        let (start, end) = (read_u32(groups, group), read_u32(groups, group + 1));
        (start..end).map(|i| self.entry(i as usize)).collect()
    }

    fn entry(&self, index: usize) -> Entry<'_> {
        let e = &self.section(Section::Entries)[index * ENTRY_SIZE..(index + 1) * ENTRY_SIZE];
        let reading_at = u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize;
        let surface = u32::from_le_bytes(e[4..8].try_into().unwrap());
        let pair = u16::from_le_bytes([e[8], e[9]]) as usize;
        let cost = i16::from_le_bytes([e[10], e[11]]);

        // 開くときに、読みと表記が正しく取り出せることを確かめてある
        let reading = item(self.section(Section::Readings), reading_at)
            .and_then(kana::decode)
            .unwrap_or_default();
        let surface = match surface & FLAG_MASK {
            FLAG_SAME_AS_READING => Cow::Owned(reading.clone()),
            FLAG_KATAKANA => Cow::Owned(to_katakana(&reading)),
            _ => Cow::Borrowed(
                item(self.section(Section::Strings), surface as usize)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or(""),
            ),
        };
        let pairs = self.section(Section::PosPairs);
        Entry {
            surface,
            reading,
            lid: read_u16(pairs, pair * 2),
            rid: read_u16(pairs, pair * 2 + 1),
            cost,
        }
    }
}

/// ひらがな（ぁ〜ゖ）をカタカナにする。
pub(crate) fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ぁ'..='ゖ' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}
