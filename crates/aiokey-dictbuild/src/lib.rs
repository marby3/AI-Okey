//! Mozc の辞書（dictionary_oss）から、スケルトン索引辞書を作る。

use std::fs;
use std::path::Path;

use aiokey_engine::dict::{build_with_report, BuildReport, SourceEntry};

/// `dir` にある Mozc の辞書（dictionary00〜09.txt）と接続コストから、辞書ファイルの中身を作る。
pub fn build_from_mozc(dir: &Path) -> Result<Vec<u8>, String> {
    build_from_mozc_with_report(dir).map(|(bytes, _)| bytes)
}

/// [`build_from_mozc`] と同じで、入れなかった語の数も返す。
///
/// 品詞の数は connection_single_column.txt の 1 行目を正とし、id.def の行数と一致するか確かめる。
pub fn build_from_mozc_with_report(dir: &Path) -> Result<(Vec<u8>, BuildReport), String> {
    let (pos_count, connection) = read_connection(&dir.join("connection_single_column.txt"))?;
    let id_def = dir.join("id.def");
    let ids = fs::read_to_string(&id_def)
        .map_err(|e| format!("{}: {e}", id_def.display()))?
        .lines()
        .filter(|l| !l.is_empty())
        .count();
    if ids != pos_count as usize {
        return Err(format!(
            "{} has {ids} ids, but the connection table has {pos_count}",
            id_def.display()
        ));
    }
    let mut entries = Vec::new();
    for i in 0..10 {
        let path = dir.join(format!("dictionary{i:02}.txt"));
        entries.extend(read_dictionary(&path)?);
    }
    build_with_report(entries, pos_count, &connection)
}

/// Mozc の辞書ファイル 1 つを読む。1 行は「読み, lid, rid, コスト, 表記」のタブ区切り。
pub fn read_dictionary(path: &Path) -> Result<Vec<SourceEntry>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(no, line)| {
            parse_line(line)
                .ok_or_else(|| format!("{}:{}: bad line: {line}", path.display(), no + 1))
        })
        .collect()
}

fn parse_line(line: &str) -> Option<SourceEntry> {
    let mut fields = line.split('\t');
    let reading = fields.next()?.to_string();
    let lid = fields.next()?.parse().ok()?;
    let rid = fields.next()?.parse().ok()?;
    let cost = fields.next()?.parse().ok()?;
    let surface = fields.next()?.to_string();
    Some(SourceEntry {
        reading,
        surface,
        lid,
        rid,
        cost,
    })
}

/// connection_single_column.txt を読み、品詞の数と N×N のコスト（行が rid、列が lid）を返す。
pub fn read_connection(path: &Path) -> Result<(u16, Vec<i16>), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    let n: u16 = lines
        .next()
        .and_then(|l| l.trim().parse().ok())
        .ok_or_else(|| format!("{}: the first line must be the pos count", path.display()))?;
    let costs = lines
        .enumerate()
        .map(|(no, l)| {
            l.trim()
                .parse::<i16>()
                .map_err(|_| format!("{}:{}: bad cost: {l}", path.display(), no + 2))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected = n as usize * n as usize;
    if costs.len() != expected {
        return Err(format!(
            "{}: {} costs, expected {expected}",
            path.display(),
            costs.len()
        ));
    }
    Ok((n, costs))
}
