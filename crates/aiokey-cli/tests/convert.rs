//! Issue #4 の受け入れテスト：段1（ラティスと N-best）。
//!
//! Mozc のデータ（`scripts/fetch-mozc.sh` で取得）から作った辞書を使う。

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_engine::skeleton::skeletonize;

fn mozc_dir() -> PathBuf {
    std::env::var_os("AIOKEY_MOZC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mozc"))
}

/// 全データから辞書を 1 回だけ作ってファイルに書き、そのパスと開いた辞書を返す。
fn dictionary() -> &'static (PathBuf, Dictionary) {
    static DICT: OnceLock<(PathBuf, Dictionary)> = OnceLock::new();
    DICT.get_or_init(|| {
        let bytes = aiokey_dictbuild::build_from_mozc(&mozc_dir())
            .expect("辞書を作れない。scripts/fetch-mozc.sh を実行したか確かめる");
        let path = std::env::temp_dir().join(format!("aiokey-convert-{}.dic", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let dict = Dictionary::open(&path).unwrap();
        (path, dict)
    })
}

fn surfaces(input: &str, k: usize) -> Vec<String> {
    let (_, dict) = dictionary();
    convert(dict, input, &Options { k })
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

#[test]
fn cli_convert_returns_hatashite_hontou_ni() {
    let (path, _) = dictionary();
    let out = Command::new(env!("CARGO_BIN_EXE_aiokey-cli"))
        .args(["convert", "htsthqtun", "--dict"])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.len() <= 32, "{} lines", lines.len());
    assert!(
        lines.iter().any(|l| l.contains("果たして本当に")),
        "{stdout}"
    );
}

#[test]
fn candidates_are_unique_sorted_and_limited_to_k() {
    let (_, dict) = dictionary();
    for k in [32, 5] {
        let candidates = convert(dict, "htsthqtun", &Options { k });
        assert!(!candidates.is_empty() && candidates.len() <= k);
        let mut seen = std::collections::HashSet::new();
        for c in &candidates {
            assert!(seen.insert(c.surface.clone()), "重複: {}", c.surface);
        }
        assert!(candidates.windows(2).all(|w| w[0].cost <= w[1].cost));
    }
    assert_eq!(Options::default().k, 32);
}

#[test]
fn candidates_carry_reading_cost_and_bunsetsu_matching_the_input() {
    let (_, dict) = dictionary();
    let input = "htsthqtun";
    let len = input.chars().count();
    for c in convert(dict, input, &Options::default()) {
        assert_eq!(
            skeletonize(&c.reading),
            input,
            "{} / {}",
            c.surface,
            c.reading
        );
        assert_eq!(
            c.words
                .iter()
                .map(|w| w.surface.as_str())
                .collect::<String>(),
            c.surface
        );
        // 文節は入力の先頭から末尾まで、すき間なく並ぶ
        assert!(!c.bunsetsu.is_empty());
        assert_eq!(c.bunsetsu.first().unwrap().start, 0);
        assert_eq!(c.bunsetsu.last().unwrap().end, len);
        assert!(c.bunsetsu.windows(2).all(|w| w[0].end == w[1].start));
    }
}

#[test]
fn vowel_in_mixed_input_narrows_candidates() {
    let s = surfaces("kik", 32);
    assert!(s.iter().any(|x| x == "聞く"), "{s:?}");
    assert!(s.iter().any(|x| x == "開花"), "{s:?}");
    assert!(!s.iter().any(|x| x == "書く"), "{s:?}");
}

#[test]
fn unknown_sequence_still_gets_a_hiragana_candidate() {
    let (_, dict) = dictionary();
    let candidates = convert(dict, "zzzz", &Options::default());
    assert!(!candidates.is_empty());
    assert!(candidates.iter().any(|c| {
        c.surface.chars().all(|ch| ('ぁ'..='ゖ').contains(&ch)) && skeletonize(&c.surface) == "zzzz"
    }));
}

#[test]
fn bench_reports_p50_and_p95() {
    let (path, _) = dictionary();
    let out = Command::new(env!("CARGO_BIN_EXE_aiokey-cli"))
        .args(["bench", "--iterations", "5", "--dict"])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("p50") && stdout.contains("p95"), "{stdout}");
}

#[test]
fn malformed_input_does_not_panic() {
    // IME から届く入力は信頼できない。想定外の文字が混ざっても落ちず、何か候補を返す
    let (_, dict) = dictionary();
    for input in ["\0", "k\0t", "漢🙂ｱ", "/", "/ /", " ", "", "qqqqxxxx----"] {
        let candidates = convert(dict, input, &Options::default());
        if !input.is_empty() {
            assert!(!candidates.is_empty(), "{input:?}");
        }
    }
}
