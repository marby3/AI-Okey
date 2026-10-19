//! 辞書ファイルを作る。

use std::collections::HashMap;

use super::format::{
    words_per_row, Section, ENTRY_SIZE, FLAG_KATAKANA, FLAG_SAME_AS_READING, HEADER_SIZE, MAGIC,
    SECTION_COUNT, VERSION,
};
use super::{kana, to_katakana};
use crate::skeleton::skeletonize;

/// 辞書に入れる 1 語。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceEntry {
    pub reading: String,
    pub surface: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: i16,
}

/// 長さ u8 + 中身の並び。同じ中身は 1 回だけ入れる。
#[derive(Default)]
struct Pool {
    bytes: Vec<u8>,
    offsets: HashMap<Vec<u8>, u32>,
}

impl Pool {
    fn add(&mut self, item: &[u8]) -> Result<u32, String> {
        if let Some(&offset) = self.offsets.get(item) {
            return Ok(offset);
        }
        let len = u8::try_from(item.len()).map_err(|_| "a string is longer than 255 bytes")?;
        let offset = u32::try_from(self.bytes.len()).map_err(|_| "a string pool is too large")?;
        self.bytes.push(len);
        self.bytes.extend_from_slice(item);
        self.offsets.insert(item.to_vec(), offset);
        Ok(offset)
    }
}

/// 語の一覧と連接コスト（`pos_count` × `pos_count`、行が rid）から、辞書ファイルの中身を作る。
///
/// 読みをスケルトンにしたものがキーになる。スケルトンが空になる語、文字列が長すぎる語、
/// 文脈 ID が `pos_count` 以上の語は入れない。
pub fn build(
    entries: impl IntoIterator<Item = SourceEntry>,
    pos_count: u16,
    connection: &[i16],
) -> Result<Vec<u8>, String> {
    let n = pos_count as usize;
    if n == 0 || connection.len() != n * n {
        return Err(format!(
            "connection has {} costs, expected {}",
            connection.len(),
            n * n
        ));
    }

    // (スケルトン, 語) に並べ、まったく同じ語は 1 つにする
    let mut keyed: Vec<(String, SourceEntry)> = entries
        .into_iter()
        .filter(|e| e.surface.len() <= 255 && kana::encode(&e.reading).len() <= 255)
        .filter(|e| (e.lid as usize) < n && (e.rid as usize) < n)
        .filter_map(|e| {
            let key = skeletonize(&e.reading);
            (!key.is_empty() && !key.contains('\0')).then_some((key, e))
        })
        .collect();
    // キーはバイト順に並べる（yada の求める順。String の順と同じ）
    keyed.sort_by(|(ka, a), (kb, b)| {
        (ka, &a.reading, a.cost, &a.surface, a.lid, a.rid)
            .cmp(&(kb, &b.reading, b.cost, &b.surface, b.lid, b.rid))
    });
    keyed.dedup();

    let mut readings = Pool::default();
    let mut strings = Pool::default();
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    let mut pair_index: HashMap<(u16, u16), u16> = HashMap::new();
    let mut groups: Vec<u32> = Vec::new();
    let mut keys: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut entry_bytes: Vec<u8> = Vec::with_capacity(keyed.len() * ENTRY_SIZE);

    for (i, (key, e)) in keyed.iter().enumerate() {
        if i == 0 || keyed[i - 1].0 != *key {
            keys.push((key.as_bytes().to_vec(), groups.len() as u32));
            groups.push(i as u32);
        }
        let reading = readings.add(&kana::encode(&e.reading))?;
        let surface = if e.surface == e.reading {
            FLAG_SAME_AS_READING
        } else if e.surface == to_katakana(&e.reading) {
            FLAG_KATAKANA
        } else {
            let offset = strings.add(e.surface.as_bytes())?;
            if offset >= FLAG_SAME_AS_READING {
                return Err("the surface pool is too large".to_string());
            }
            offset
        };
        let pair = match pair_index.get(&(e.lid, e.rid)) {
            Some(&p) => p,
            None => {
                let p = u16::try_from(pairs.len()).map_err(|_| "too many pos pairs")?;
                pairs.push((e.lid, e.rid));
                pair_index.insert((e.lid, e.rid), p);
                p
            }
        };
        entry_bytes.extend_from_slice(&reading.to_le_bytes());
        entry_bytes.extend_from_slice(&surface.to_le_bytes());
        entry_bytes.extend_from_slice(&pair.to_le_bytes());
        entry_bytes.extend_from_slice(&e.cost.to_le_bytes());
    }
    groups.push(keyed.len() as u32);

    let trie = yada::builder::DoubleArrayBuilder::build(&keys)
        .ok_or_else(|| "failed to build the double-array trie".to_string())?;

    let mut bodies: [Vec<u8>; SECTION_COUNT] = Default::default();
    bodies[Section::Trie as usize] = trie;
    bodies[Section::Groups as usize] = groups.iter().flat_map(|x| x.to_le_bytes()).collect();
    bodies[Section::Entries as usize] = entry_bytes;
    bodies[Section::Readings as usize] = readings.bytes;
    bodies[Section::Strings as usize] = strings.bytes;
    bodies[Section::PosPairs as usize] = pairs
        .iter()
        .flat_map(|(l, r)| [l.to_le_bytes(), r.to_le_bytes()].concat())
        .collect();
    let [modes, bits, ranks, values] = compress_connection(pos_count, connection);
    bodies[Section::ConnModes as usize] = modes;
    bodies[Section::ConnBits as usize] = bits;
    bodies[Section::ConnRanks as usize] = ranks;
    bodies[Section::ConnValues as usize] = values;

    let mut out = vec![0u8; HEADER_SIZE];
    out[0..8].copy_from_slice(MAGIC);
    out[8..12].copy_from_slice(&VERSION.to_le_bytes());
    out[12..14].copy_from_slice(&pos_count.to_le_bytes());
    for (i, body) in bodies.iter().enumerate() {
        // 各部分を 8 バイト境界から始める
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        let offset = u32::try_from(out.len()).map_err(|_| "dictionary is too large")?;
        let len = u32::try_from(body.len()).map_err(|_| "a section is too large")?;
        out[24 + i * 8..28 + i * 8].copy_from_slice(&offset.to_le_bytes());
        out[28 + i * 8..32 + i * 8].copy_from_slice(&len.to_le_bytes());
        out.extend_from_slice(body);
    }
    let crc = crc32fast::hash(&out[HEADER_SIZE..]);
    out[16..20].copy_from_slice(&crc.to_le_bytes());
    Ok(out)
}

/// 連接コストを、行ごとの最頻値と、それと違うセルだけの値に分ける（Mozc と同じ考え方）。
fn compress_connection(pos_count: u16, connection: &[i16]) -> [Vec<u8>; 4] {
    let n = pos_count as usize;
    let wpr = words_per_row(pos_count);
    let mut modes = Vec::with_capacity(n * 2);
    let mut bits = vec![0u64; n * wpr];
    let mut values = Vec::new();
    for rid in 0..n {
        let row = &connection[rid * n..(rid + 1) * n];
        let mut counts: HashMap<i16, usize> = HashMap::new();
        for &c in row {
            *counts.entry(c).or_default() += 1;
        }
        // 同数なら小さい値を最頻値にする（毎回同じ結果になるように）
        let mode = counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(&c, _)| c)
            .unwrap_or(0);
        modes.extend_from_slice(&mode.to_le_bytes());
        for (lid, &c) in row.iter().enumerate() {
            if c != mode {
                bits[rid * wpr + lid / 64] |= 1 << (lid % 64);
                values.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    let mut ranks = Vec::with_capacity(bits.len() * 4);
    let mut count = 0u32;
    for w in &bits {
        ranks.extend_from_slice(&count.to_le_bytes());
        count += w.count_ones();
    }
    let bits = bits.iter().flat_map(|w| w.to_le_bytes()).collect();
    [modes, bits, ranks, values]
}
