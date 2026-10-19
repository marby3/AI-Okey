//! Mozc の辞書（dictionary_oss）から、スケルトン索引辞書を作る。

use std::fs;
use std::path::Path;

use aiokey_engine::dict::{build_with_report, BuildReport, PosTable, SourceEntry};

/// `dir` にある Mozc の辞書（dictionary00〜09.txt）と接続コスト、同梱の英単語から、辞書ファイルの中身を作る。
pub fn build_from_mozc(dir: &Path) -> Result<Vec<u8>, String> {
    build_from_mozc_with_report(dir).map(|(bytes, _)| bytes)
}

/// [`build_from_mozc`] と同じで、入れなかった語の数も返す。
///
/// 品詞の数は connection_single_column.txt の 1 行目を正とし、id.def の行数と一致するか確かめる。
pub fn build_from_mozc_with_report(dir: &Path) -> Result<(Vec<u8>, BuildReport), String> {
    let (pos_count, connection) = read_connection(&dir.join("connection_single_column.txt"))?;
    let pos = read_id_def(&dir.join("id.def"))?;
    if pos.count != pos_count {
        return Err(format!(
            "id.def has {} ids, but the connection table has {pos_count}",
            pos.count
        ));
    }
    let mut entries = Vec::new();
    for i in 0..10 {
        let path = dir.join(format!("dictionary{i:02}.txt"));
        entries.extend(read_dictionary(&path)?);
    }
    let english = parse_english_tsv(ENGLISH_SEED, "english-seed.tsv")?;
    entries.extend(english_entries(&english, pos.unknown));
    build_with_report(entries, &pos, &connection)
}

/// 未知語に使う品詞の名前。
const UNKNOWN_POS: &str = "名詞,一般,*,*,*,*,*";

/// id.def を読む。1 行は「ID 品詞名」で、ID は 0 から順に並ぶ。
///
/// 付属語の印は、品詞名から近似で決める（助詞・助動詞と、接尾・非自立の語）。
/// Mozc の文節区切り（segmenter）ほど細かくはない。
pub fn read_id_def(path: &Path) -> Result<PosTable, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut function_word = Vec::new();
    let mut unknown = None;
    for (no, line) in text.lines().filter(|l| !l.is_empty()).enumerate() {
        let (id, name) = line
            .split_once(' ')
            .ok_or_else(|| format!("{}:{}: bad line: {line}", path.display(), no + 1))?;
        if id.parse::<usize>().ok() != Some(no) {
            return Err(format!(
                "{}:{}: ids must be 0, 1, 2, ...",
                path.display(),
                no + 1
            ));
        }
        let fields: Vec<&str> = name.split(',').collect();
        let is_function = matches!(fields[0], "助詞" | "助動詞")
            || fields
                .get(1)
                .is_some_and(|f| matches!(*f, "接尾" | "非自立"));
        function_word.push(is_function);
        if name == UNKNOWN_POS && unknown.is_none() {
            unknown = Some(no as u16);
        }
    }
    let count = u16::try_from(function_word.len()).map_err(|_| "too many ids in id.def")?;
    let unknown = unknown.ok_or_else(|| format!("{}: no {UNKNOWN_POS}", path.display()))?;
    Ok(PosTable {
        count,
        unknown,
        function_word,
    })
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

/// ビルダーに同梱した英単語の頻度（#6 でコーパスから作るまでのつなぎ）。
const ENGLISH_SEED: &str = include_str!("../data/english-seed.tsv");

/// 英単語の頻度 TSV を読む。1 行は「つづり<TAB>頻度」。空行と `#` で始まる行は読み飛ばす。
pub fn read_english_tsv(path: &Path) -> Result<Vec<(String, u32)>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_english_tsv(&text, &path.display().to_string())
}

fn parse_english_tsv(text: &str, name: &str) -> Result<Vec<(String, u32)>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|(no, line)| {
            let bad = || format!("{name}:{}: expected \"word<TAB>frequency\": {line}", no + 1);
            let (word, freq) = line.split_once('\t').ok_or_else(bad)?;
            let freq: u32 = freq.trim().parse().map_err(|_| bad())?;
            let word = word.trim();
            if word.is_empty() {
                return Err(bad());
            }
            Ok((word.to_string(), freq))
        })
        .collect()
}

/// 英単語を辞書の語にする。見出し（読み）は小文字のつづりで、表記はつづりのまま。
///
/// 入力パーサーは q・x・- もスケルトンの文字として読むので、英字を素直にたどった道のキーは
/// つづりと同じになり、英単語は日本語の語と同じ辞書・同じ検索で見つかる。
/// 英字と `-` だけの語を入れる（ほかの文字を含む語は、入力からたどり着けないので入れない）。
/// コストは頻度が高いほど小さくする。品詞は未知語と同じ（名詞,一般）。
pub fn english_entries(words: &[(String, u32)], pos: u16) -> Vec<SourceEntry> {
    words
        .iter()
        .filter(|(w, _)| w.chars().all(|c| c.is_ascii_alphabetic() || c == '-'))
        .map(|(word, freq)| SourceEntry {
            reading: word.to_ascii_lowercase(),
            surface: word.clone(),
            lid: pos,
            rid: pos,
            cost: english_cost(*freq),
        })
        .collect()
}

/// 頻度から単語コストを決める（Mozc の語のコストはおおむね 3000〜9000）。
fn english_cost(freq: u32) -> i16 {
    let cost = 8000.0 - 600.0 * (freq.max(1) as f64).ln();
    cost.clamp(2500.0, 9000.0) as i16
}
