//! Issue #3 の受け入れテスト：スケルトン索引辞書ビルダー。
//!
//! Mozc のデータ（`scripts/fetch-mozc.sh` で取得）を使う。置き場所は環境変数
//! `AIOKEY_MOZC_DIR`、なければリポジトリの `data/mozc`。

use std::path::PathBuf;
use std::sync::OnceLock;

use aiokey_dictbuild::build_from_mozc;
use aiokey_engine::dict::Dictionary;

fn mozc_dir() -> PathBuf {
    std::env::var_os("AIOKEY_MOZC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mozc"))
}

/// 全データから辞書を 1 回だけ作り、ファイルに書いたものを mmap で開く。
fn dictionary() -> &'static (PathBuf, Dictionary) {
    static DICT: OnceLock<(PathBuf, Dictionary)> = OnceLock::new();
    DICT.get_or_init(|| {
        let dir = mozc_dir();
        assert!(
            dir.join("id.def").exists(),
            "Mozc のデータがない（{}）。scripts/fetch-mozc.sh を実行する",
            dir.display()
        );
        let bytes = build_from_mozc(&dir).expect("build");
        let path = std::env::temp_dir().join(format!("aiokey-test-{}.dic", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let dict = Dictionary::open(&path).expect("open");
        (path, dict)
    })
}

#[test]
fn lookup_htst_returns_hatashite() {
    let (_, dict) = dictionary();
    let entries = dict.lookup("htst");
    assert!(
        entries
            .iter()
            .any(|e| e.surface == "果たして" && e.reading == "はたして"),
        "{:?}",
        entries
            .iter()
            .map(|e| e.surface.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn common_prefix_search_returns_every_length() {
    let (_, dict) = dictionary();
    let lengths: Vec<usize> = dict
        .common_prefix_search("htsthqtun")
        .iter()
        .map(|(len, entries)| {
            assert!(!entries.is_empty());
            *len
        })
        .collect();
    for len in 1..=4 {
        assert!(lengths.contains(&len), "長さ {len} がない: {lengths:?}");
    }
}

#[test]
fn connection_cost_matches_mozc_source() {
    let (_, dict) = dictionary();
    assert_eq!(dict.pos_count(), 2672);
    // 期待値は connection_single_column.txt の (2 + rid × 2672 + lid) 行目を、ビルダーの
    // 解析を通さずに書き写したもの。行が rid であることは Mozc の gen_connection_data.py
    // （rid = array_index // pos_size）による。12 は副詞,一般、1851 は名詞,一般（id.def）。
    for (rid, lid, expected) in [
        (0u16, 1u16, 5940i16),
        (12, 12, 2132),
        (12, 1851, 894),
        (1851, 12, 3759),
        (5, 900, 7401),
        (2671, 2671, 3776),
    ] {
        assert_eq!(dict.cost(rid, lid), expected, "rid={rid} lid={lid}");
    }
}

#[test]
fn dictionary_is_opened_with_mmap_and_size_is_reported() {
    let (path, dict) = dictionary();
    let file_size = std::fs::metadata(path).unwrap().len() as usize;
    assert!(dict.is_mmapped());
    assert_eq!(dict.size_bytes(), file_size);
    // サイズは記録用に出しておく（暫定目標 75MB 以下）
    println!(
        "dictionary size: {} bytes ({:.1} MB)",
        file_size,
        file_size as f64 / 1e6
    );
}

#[test]
fn corrupted_dictionary_file_is_rejected() {
    // 辞書ファイルは IME の外から差し替えられうる。壊れていたら開かずにエラーにする
    let (path, _) = dictionary();
    let mut bytes = std::fs::read(path).unwrap();

    let truncated = bytes[..bytes.len() / 2].to_vec();
    assert!(Dictionary::from_bytes(truncated).is_err());

    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    assert!(Dictionary::from_bytes(bytes).is_err());

    assert!(Dictionary::from_bytes(b"not a dictionary".to_vec()).is_err());

    // ヘッダーの部分表（トライの長さ）だけを、範囲内に収まるように書き換えたものも開かない
    let mut header_edited = std::fs::read(path).unwrap();
    header_edited[28] ^= 0x08;
    assert!(Dictionary::from_bytes(header_edited).is_err());
}
