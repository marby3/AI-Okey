//! Issue #58 基準 2：段1の候補を文字 n-gram で並べ替える。
//!
//! Mozc のデータ（`scripts/fetch-mozc.sh` で取得）から辞書を作る。

use std::path::PathBuf;
use std::sync::OnceLock;

use aiokey_engine::convert::{convert, convert_reranked, Options, Rerank};
use aiokey_engine::dict::Dictionary;
use aiokey_engine::ngram::{to_bytes, Ngram};

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

/// 「ふ」の続きを安くする表（段1の 1 位は「果たして」、「ふとした」が後ろにいる入力で試す）
fn table() -> Ngram {
    let entries = vec![("ふ".to_string(), 1), ("と".to_string(), 1)];
    Ngram::from_bytes(to_bytes(entries, 1000).unwrap()).unwrap()
}

fn surfaces(input: &str, k: usize, rerank: Option<&Rerank>) -> Vec<String> {
    convert_reranked(dictionary(), input, &Options { k }, rerank)
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

#[test]
fn candidates_are_reranked_by_cost_and_ngram_and_keep_the_required_ones() {
    let ngram = table();
    let rerank = Rerank {
        ngram: &ngram,
        pool: 128,
        cost_per_nat: 250,
        max_entries_per_span: 40,
    };

    // 表を渡さなければ、今の並びと完全に同じ
    for input in ["htst", "htsthqtun", "zyq/F/knd", "wtsh/Python/wtkims"] {
        let plain: Vec<String> = convert(dictionary(), input, &Options { k: 32 })
            .into_iter()
            .map(|c| c.surface)
            .collect();
        assert_eq!(surfaces(input, 32, None), plain, "{input}");
    }

    // 上位 N 件を探してから並べ替え、上位 K 件を返す：32 件の外の候補も先頭に上がれる
    let plain = surfaces("htsthqtun", 128, None);
    let reranked = surfaces("htsthqtun", 32, Some(&rerank));
    assert_eq!(reranked.len(), 32);
    assert_ne!(reranked[..9], plain[..9]);
    let raised = reranked
        .iter()
        .take(31)
        .any(|s| plain.iter().position(|p| p == s).is_some_and(|i| i >= 32));
    assert!(raised, "{reranked:?}");

    // 打った入力そのものの候補は必ず残る
    assert!(reranked.iter().any(|s| s == "htsthqtun"));

    // 文の途中の `/` では、英語の指定と「・」の候補がどちらも上位 5 件に入る（#47、#52）
    let slash = surfaces("htst/hqt", 32, Some(&rerank));
    assert!(slash[..5].iter().any(|s| s.contains("hqt") && !s.contains('・')), "{slash:?}");
    assert!(slash[..5].iter().any(|s| s.contains('・')), "{slash:?}");
}
