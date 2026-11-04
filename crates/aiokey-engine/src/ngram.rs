//! 文字 n-gram の表（段1の候補の並べ替えに使う。#58）。
//!
//! 表は `python -m aiokey ngram` がコーパスから作り、[`to_bytes`] の形式で書く。1 つのファイルで、mmap で開いてそのまま使う。
//! 持つのは 3 文字までの並びと、その負の対数確率（centinat、0.01 ナット）だけで、文章そのものは入らない。
//!
//! 形式（リトルエンディアン）：
//!
//! ```text
//! magic "AIOKNGR1" | crc32（これより後ろ全部） | n1 | n2 | n3 | 知らない文字のコスト（すべて u32）
//! n1 個の u64 キー | n1 個の u16 コスト | n2 個の u64 キー | n2 個の u16 コスト | n3 個の u64 キー | n3 個の u16 コスト
//! ```
//!
//! キーは文字のコードポイントを 21 ビットずつ詰めた値で、n ごとに昇順に並ぶ。文頭は `\0` で表す。

use std::fmt;
use std::path::Path;

const MAGIC: &[u8; 8] = b"AIOKNGR1";
const HEADER: usize = 8 + 4 * 5;
/// 表が持つ並びの長さの上限。4 文字以上の並びは書けない。
pub const MAX_CHARS: usize = 3;
const BOS: char = '\0';

/// 表を開けなかった、または作れなかった理由。
#[derive(Debug)]
pub struct NgramError(pub String);

impl fmt::Display for NgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid ngram table: {}", self.0)
    }
}

impl std::error::Error for NgramError {}

enum Data {
    Mmap(memmap2::Mmap),
    Owned(Vec<u8>),
}

impl std::ops::Deref for Data {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Data::Mmap(m) => m,
            Data::Owned(v) => v,
        }
    }
}

/// n ごとのキーとコストの位置と数。
#[derive(Clone, Copy)]
struct Part {
    keys: usize,
    costs: usize,
    len: usize,
}

/// 文字 n-gram の表。
pub struct Ngram {
    data: Data,
    parts: [Part; MAX_CHARS],
    unknown: u32,
}

impl fmt::Debug for Ngram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ngram")
            .field("entries", &self.parts.map(|p| p.len))
            .finish()
    }
}

impl Ngram {
    /// ファイルを mmap で開く。
    pub fn open(path: &Path) -> Result<Self, NgramError> {
        let file = std::fs::File::open(path)
            .map_err(|e| NgramError(format!("{}: {e}", path.display())))?;
        // SAFETY: 開いたあとで外からファイルを書き換えられると、読む中身が変わる。辞書と同じく、インストール先に置く
        // 読み取り専用のファイルとして扱う。
        let mmap = unsafe { memmap2::Mmap::map(&file) }
            .map_err(|e| NgramError(format!("{}: {e}", path.display())))?;
        Self::new(Data::Mmap(mmap))
    }

    /// メモリ上のバイト列から開く（テスト向け）。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, NgramError> {
        Self::new(Data::Owned(bytes))
    }

    fn new(data: Data) -> Result<Self, NgramError> {
        if data.len() < HEADER || &data[..8] != MAGIC {
            return Err(NgramError("bad magic or too short".into()));
        }
        let word = |i: usize| u32::from_le_bytes(data[8 + 4 * i..12 + 4 * i].try_into().unwrap());
        if word(0) != crc32fast::hash(&data[12..]) {
            return Err(NgramError("checksum mismatch".into()));
        }
        let mut parts = [Part {
            keys: 0,
            costs: 0,
            len: 0,
        }; MAX_CHARS];
        let mut at = HEADER;
        for (i, part) in parts.iter_mut().enumerate() {
            let len = word(1 + i) as usize;
            *part = Part {
                keys: at,
                costs: at + 8 * len,
                len,
            };
            at += 10 * len;
        }
        if at != data.len() {
            return Err(NgramError("size does not match the header".into()));
        }
        let unknown = word(4);
        Ok(Self {
            data,
            parts,
            unknown,
        })
    }

    fn get(&self, n: usize, key: u64) -> Option<u32> {
        let part = self.parts[n - 1];
        let (mut lo, mut hi) = (0, part.len);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = part.keys + 8 * mid;
            let k = u64::from_le_bytes(self.data[at..at + 8].try_into().unwrap());
            match k.cmp(&key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let at = part.costs + 2 * mid;
                    let c = u16::from_le_bytes(self.data[at..at + 2].try_into().unwrap());
                    return Some(c as u32);
                }
            }
        }
        None
    }

    /// 文の負の対数確率（centinat）。3 文字の並びがなければ 2 文字に、それもなければ 1 文字に戻る。
    pub fn cost(&self, text: &str) -> u64 {
        let (mut a, mut b) = (BOS, BOS);
        let mut total = 0;
        for c in text.chars() {
            let cost = self
                .get(3, key(&[a, b, c]))
                .or_else(|| self.get(2, key(&[b, c])))
                .or_else(|| self.get(1, key(&[c])))
                .unwrap_or(self.unknown);
            total += cost as u64;
            (a, b) = (b, c);
        }
        total
    }
}

fn key(chars: &[char]) -> u64 {
    chars.iter().fold(0, |k, &c| (k << 21) | c as u64)
}

/// (並び, コスト) の一覧と、知らない文字のコストから、表のファイルの中身を作る。並びは 1〜[`MAX_CHARS`] 文字。
pub fn to_bytes(
    entries: impl IntoIterator<Item = (String, u16)>,
    unknown: u32,
) -> Result<Vec<u8>, NgramError> {
    let mut parts: [Vec<(u64, u16)>; MAX_CHARS] = Default::default();
    for (text, cost) in entries {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() || chars.len() > MAX_CHARS {
            return Err(NgramError(format!(
                "{text:?}: must be 1 to {MAX_CHARS} characters"
            )));
        }
        parts[chars.len() - 1].push((key(&chars), cost));
    }
    let mut body = Vec::new();
    for part in &mut parts {
        part.sort_unstable();
        if part.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(NgramError("duplicate entries".into()));
        }
    }
    for part in &parts {
        body.extend((part.len() as u32).to_le_bytes());
    }
    body.extend(unknown.to_le_bytes());
    for part in &parts {
        body.extend(part.iter().flat_map(|(k, _)| k.to_le_bytes()));
        body.extend(part.iter().flat_map(|(_, c)| c.to_le_bytes()));
    }
    let mut out = Vec::with_capacity(12 + body.len());
    out.extend(MAGIC);
    out.extend(crc32fast::hash(&body).to_le_bytes());
    out.extend(body);
    Ok(out)
}
