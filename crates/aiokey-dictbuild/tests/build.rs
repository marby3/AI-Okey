//! Issue #3 の受け入れテスト：スケルトン索引辞書ビルダー。
//!
//! Mozc のデータ（`scripts/fetch-mozc.sh` で取得）を使う。置き場所は環境変数
//! `AIOKEY_MOZC_DIR`、なければリポジトリの `data/mozc`。

use std::path::PathBuf;
use std::sync::OnceLock;

use aiokey_dictbuild::{build_from_mozc, read_connection};
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
    let (n, costs) = read_connection(&mozc_dir().join("connection_single_column.txt")).unwrap();
    assert_eq!(dict.pos_count(), n);
    // 先頭・末尾と、はたして（品詞 12）の前後をいくつか比べる
    for (rid, lid) in [
        (0u16, 1u16),
        (12, 12),
        (12, 1851),
        (1851, 12),
        (n - 1, n - 1),
    ] {
        let expected = costs[rid as usize * n as usize + lid as usize];
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
}
