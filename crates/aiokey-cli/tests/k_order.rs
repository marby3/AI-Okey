//! Issue #30 の受け入れテスト：K を変えても段1の上位の並びが変わらない。
//!
//! Mozc のデータ（`scripts/fetch-mozc.sh` で取得）から辞書を作る。

use std::path::PathBuf;
use std::sync::OnceLock;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;

fn mozc_dir() -> PathBuf {
    std::env::var_os("AIOKEY_MOZC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mozc"))
}

fn dictionary() -> &'static Dictionary {
    static DICT: OnceLock<Dictionary> = OnceLock::new();
    DICT.get_or_init(|| {
        let bytes = aiokey_dictbuild::build_from_mozc(&mozc_dir())
            .expect("辞書を作れない。scripts/fetch-mozc.sh を実行したか確かめる");
        Dictionary::from_bytes(bytes).unwrap()
    })
}

fn surfaces(input: &str, k: usize) -> Vec<String> {
    convert(dictionary(), input, &Options { k })
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

// szqkqkyu（自然環境）と hgasigw（東側）は、K = 1 で打った英字が 1 位になっていた例。
// 英語指定の `/` を含む入力は、候補が `/` を除いた文字列になるので、ここには入れない
const INPUTS: &[&str] = &["htsthqtun", "kik", "szqkqkyu", "hgasigw", "knkeyboardds"];

#[test]
fn top1_with_k1_is_the_same_as_with_k32() {
    for input in INPUTS {
        let best = surfaces(input, 32);
        // K = 32 の 1 位が打った文字列そのものだと、この入力では不具合を再現できない
        assert_ne!(
            best[0], *input,
            "{input}: K = 32 の 1 位が打った文字列そのもの"
        );
        assert_eq!(surfaces(input, 1), vec![best[0].clone()], "{input}");
    }
}

#[test]
fn raw_input_stays_and_the_rest_keeps_the_k32_order() {
    for input in INPUTS {
        let full: Vec<String> = surfaces(input, 32)
            .into_iter()
            .filter(|s| s != input)
            .collect();
        for k in [2, 8, 32] {
            let s = surfaces(input, k);
            assert!(s.len() <= k, "{input} (k={k}): {s:?}");
            assert!(s.iter().any(|x| x == input), "{input} (k={k}): {s:?}");
            let rest: Vec<String> = s.into_iter().filter(|x| x != input).collect();
            assert!(rest.len() <= full.len(), "{input} (k={k}): {rest:?}");
            assert_eq!(rest, full[..rest.len()], "{input} (k={k})");
        }
    }
}
