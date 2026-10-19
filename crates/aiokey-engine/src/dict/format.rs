//! 辞書ファイルの形式。数値はすべてリトルエンディアン。
//!
//! ```text
//! ヘッダー（24 + 8 × 部分の数 バイト）
//!   0  magic        "AIOKDIC\0"
//!   8  version      u32
//!  12  pos_count    u16  品詞（文脈 ID）の数 N
//!  14  reserved     u16
//!  16  crc32        u32  この欄（0 とみなす）を含むファイル全体の CRC32
//!  20  reserved     u32
//!  24  sections     (offset u32, len u32) × 部分の数 … 下の順
//!
//! Trie        yada のダブル配列。キーはスケルトン、値はグループ番号
//! Groups      u32 × (グループ数 + 1)。グループ g の語は Entries[Groups[g]..Groups[g+1]]
//! Entries     12 バイト × 語数: 読み u32, 表記 u32, 品詞の組 u16, cost i16
//!             読みは Readings の位置。表記の上位 2 ビットはフラグ
//!             （0: Strings の位置、1: 読みと同じ、2: 読みのカタカナ）
//! Readings    長さ u8 + 読み（kana.rs の符号化）、の並び
//! Strings     長さ u8 + 表記（UTF-8）、の並び
//! PosPairs    (lid u16, rid u16) × 品詞の組の数
//! ConnModes   i16 × N。各行（rid）でいちばん多い連接コスト
//! ConnBits    u64 × N × W（W = ceil(N / 64)）。最頻値と違うセルに 1 を立てる
//! ConnRanks   u32 × N × W。その語より前に立っている 1 の数
//! ConnValues  i16 × 立っている 1 の数。最頻値と違うセルの値（行優先の順）
//! ```

use super::{kana, DictError};

pub(crate) const MAGIC: &[u8; 8] = b"AIOKDIC\0";
pub(crate) const VERSION: u32 = 1;
pub(crate) const ENTRY_SIZE: usize = 12;

pub(crate) const FLAG_MASK: u32 = 0xC000_0000;
pub(crate) const FLAG_SAME_AS_READING: u32 = 0x4000_0000;
pub(crate) const FLAG_KATAKANA: u32 = 0x8000_0000;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Section {
    Trie,
    Groups,
    Entries,
    Readings,
    Strings,
    PosPairs,
    ConnModes,
    ConnBits,
    ConnRanks,
    ConnValues,
}

pub(crate) const SECTION_NAMES: [&str; SECTION_COUNT] = [
    "trie",
    "groups",
    "entries",
    "readings",
    "strings",
    "pos_pairs",
    "conn_modes",
    "conn_bits",
    "conn_ranks",
    "conn_values",
];
pub(crate) const SECTION_COUNT: usize = 10;
pub(crate) const HEADER_SIZE: usize = 24 + 8 * SECTION_COUNT;

/// 連接コストの 1 行が使う u64 の数。
pub(crate) fn words_per_row(pos_count: u16) -> usize {
    (pos_count as usize).div_ceil(64)
}

#[derive(Debug, Clone)]
pub(crate) struct Header {
    pub pos_count: u16,
    pub sections: [(u32, u32); SECTION_COUNT],
}

pub(crate) fn read_u16(bytes: &[u8], index: usize) -> u16 {
    let at = index * 2;
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

pub(crate) fn read_u32(bytes: &[u8], index: usize) -> u32 {
    let at = index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub(crate) fn read_u64(bytes: &[u8], index: usize) -> u64 {
    let at = index * 8;
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn err(msg: impl Into<String>) -> DictError {
    DictError(msg.into())
}

/// 長さ u8 + 中身の並びの、`at` にある 1 つを返す。
pub(crate) fn item(pool: &[u8], at: usize) -> Option<&[u8]> {
    let len = *pool.get(at)? as usize;
    pool.get(at + 1..at + 1 + len)
}

/// ファイル全体の CRC32。CRC の欄そのもの（16..20）は 0 とみなす。
pub(crate) fn checksum(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&data[..16]);
    hasher.update(&[0; 4]);
    hasher.update(&data[20..]);
    hasher.finalize()
}

/// ヘッダーと各部分を確かめる。
///
/// 偶然の破損（書き込みの途中で切れた、ビットが化けた）は CRC32 で見つける。そのうえで、
/// トライ以外の部分は、読む側が範囲外を指さないことまで確かめる。トライ（yada）の中身は
/// 確かめきれないので、辞書はインストール先に置く信頼できるファイルとして扱う
/// （意図的な書き換えからは、DLL と同じく OS のファイルの権限で守る）。
pub(crate) fn validate(data: &[u8]) -> Result<Header, DictError> {
    if data.len() < HEADER_SIZE || &data[0..8] != MAGIC {
        return Err(err("not an AI-O Key dictionary"));
    }
    let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
    if version != VERSION {
        return Err(err(format!("unsupported version {version}")));
    }
    let pos_count = u16::from_le_bytes([data[12], data[13]]);
    if pos_count == 0 {
        return Err(err("pos_count is 0"));
    }
    let crc = u32::from_le_bytes(data[16..20].try_into().unwrap());
    if checksum(data) != crc {
        return Err(err(
            "checksum mismatch (the file is corrupted or truncated)",
        ));
    }

    let mut sections = [(0u32, 0u32); SECTION_COUNT];
    for (i, section) in sections.iter_mut().enumerate() {
        let offset = read_u32(&data[24..], i * 2);
        let len = read_u32(&data[24..], i * 2 + 1);
        if (offset as usize) < HEADER_SIZE || offset as u64 + len as u64 > data.len() as u64 {
            return Err(err(format!("section {} is out of range", SECTION_NAMES[i])));
        }
        *section = (offset, len);
    }
    let header = Header {
        pos_count,
        sections,
    };
    let get = |s: Section| {
        let (offset, len) = header.sections[s as usize];
        &data[offset as usize..(offset + len) as usize]
    };

    let trie = get(Section::Trie);
    if trie.len() < 4 || trie.len() % 4 != 0 {
        return Err(err("trie is not a list of u32 units"));
    }

    validate_connection(pos_count, &get)?;

    let pairs = get(Section::PosPairs);
    if pairs.len() % 4 != 0 {
        return Err(err("pos_pairs is not a list of (u16, u16)"));
    }
    let pair_count = pairs.len() / 4;
    if (0..pair_count * 2).any(|i| read_u16(pairs, i) >= pos_count) {
        return Err(err("a context id is out of range"));
    }

    let reading_starts = item_starts(get(Section::Readings), kana::is_valid)?;
    let string_starts = item_starts(get(Section::Strings), |b| std::str::from_utf8(b).is_ok())?;
    let entries = get(Section::Entries);
    if entries.len() % ENTRY_SIZE != 0 {
        return Err(err("entries has a partial entry"));
    }
    let entry_count = entries.len() / ENTRY_SIZE;
    for e in entries.as_chunks::<ENTRY_SIZE>().0 {
        let reading = u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize;
        let surface = u32::from_le_bytes(e[4..8].try_into().unwrap());
        let pair = u16::from_le_bytes([e[8], e[9]]) as usize;
        if !is_start(&reading_starts, reading) {
            return Err(err("an entry points into the middle of a reading"));
        }
        match surface & FLAG_MASK {
            0 if is_start(&string_starts, surface as usize) => {}
            0 => return Err(err("an entry points into the middle of a surface")),
            FLAG_SAME_AS_READING | FLAG_KATAKANA => {}
            _ => return Err(err("an entry has an unknown surface flag")),
        }
        if pair >= pair_count {
            return Err(err("an entry has a pos pair out of range"));
        }
    }

    let groups = get(Section::Groups);
    if groups.len() % 4 != 0 || groups.len() < 4 {
        return Err(err("groups is not a list of u32"));
    }
    let mut previous = 0;
    for i in 0..groups.len() / 4 {
        let start = read_u32(groups, i);
        if start < previous || start as usize > entry_count {
            return Err(err("groups are not in order"));
        }
        previous = start;
    }
    if previous as usize != entry_count {
        return Err(err("groups do not cover every entry"));
    }

    Ok(header)
}

/// 長さ u8 + 中身の並びを先頭からたどり、各要素の開始位置を 1 にしたビット列を返す。
/// 中身が `valid` を満たさない、または並びが途中で切れていればエラー。
fn item_starts(pool: &[u8], valid: impl Fn(&[u8]) -> bool) -> Result<Vec<u64>, DictError> {
    let mut starts = vec![0u64; pool.len().div_ceil(64)];
    let mut at = 0;
    while at < pool.len() {
        let bytes = item(pool, at).ok_or_else(|| err("a string runs past the end"))?;
        if !valid(bytes) {
            return Err(err("a string is not well-formed"));
        }
        starts[at / 64] |= 1 << (at % 64);
        at += 1 + bytes.len();
    }
    Ok(starts)
}

fn is_start(starts: &[u64], at: usize) -> bool {
    starts
        .get(at / 64)
        .is_some_and(|w| w & (1 << (at % 64)) != 0)
}

fn validate_connection<'a>(
    pos_count: u16,
    get: &impl Fn(Section) -> &'a [u8],
) -> Result<(), DictError> {
    let n = pos_count as usize;
    let words = n * words_per_row(pos_count);
    if get(Section::ConnModes).len() != n * 2
        || get(Section::ConnBits).len() != words * 8
        || get(Section::ConnRanks).len() != words * 4
    {
        return Err(err("connection sections do not match pos_count"));
    }
    // 順位表を数え直して、値の数と合っているか確かめる
    let (bits, ranks) = (get(Section::ConnBits), get(Section::ConnRanks));
    let mut count = 0u32;
    for w in 0..words {
        if read_u32(ranks, w) != count {
            return Err(err("connection ranks are inconsistent"));
        }
        count += read_u64(bits, w).count_ones();
    }
    if get(Section::ConnValues).len() != count as usize * 2 {
        return Err(err("connection values do not match the bit list"));
    }
    Ok(())
}
