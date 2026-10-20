//! Issue #8 の受け入れテスト：段2の AI 採点器。
//!
//! zenz-v2.5-xsmall の GGUF（`scripts/fetch-zenz.sh` で作る）を使う。置き場所は環境変数
//! `AIOKEY_ZENZ_GGUF`、なければリポジトリの data/models/zenz-v2.5-xsmall-q8_0.gguf。

use std::path::PathBuf;

use aiokey_scorer::{prompt, Config, PromptFormat, Request, Scorer};

fn model() -> PathBuf {
    std::env::var_os("AIOKEY_ZENZ_GGUF").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/models/zenz-v2.5-xsmall-q8_0.gguf")
    })
}

/// 量子化しない（f32 の）GGUF。共有方式の答え合わせに使う。
fn model_f32() -> PathBuf {
    std::env::var_os("AIOKEY_ZENZ_F32_GGUF").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/models/zenz-v2.5-xsmall-f32.gguf")
    })
}

fn open(path: &PathBuf, format: PromptFormat) -> Scorer {
    Scorer::open(path, &Config { threads: 2, format, ..Config::default() })
        .expect("zenz の GGUF を開けない。scripts/fetch-zenz.sh を実行したか確かめる")
}

fn scorer(format: PromptFormat) -> Scorer {
    open(&model(), format)
}

const CANDIDATES: &[(&str, &str)] = &[
    ("果たして本当に", "はたしてほんとうに"),
    ("はたして本当に", "はたしてほんとうに"),
    ("果たすと本当に", "はたすとほんとうに"),
    ("浸して本当に", "ひたしてほんとうに"),
    ("果たして本当の", "はたしてほんとうの"),
];

fn request<'a>(input: &'a str, context: &'a str) -> Request<'a> {
    Request {
        context,
        input,
        candidates: CANDIDATES.to_vec(),
    }
}

#[test]
fn shared_kv_matches_scoring_each_candidate_alone() {
    // 量子化（Q8_0）の行列積は、バッチの大きさでカーネルが変わり、同じ計算でも 0.1 程度ずれる。
    // ここで確かめたいのは KV の共有とトライの正しさなので、量子化しない f32 のモデルで比べる。
    for format in [PromptFormat::ZenzV25, PromptFormat::Aiokey] {
        let mut s = open(&model_f32(), format);
        let req = request("htsthqtun", "そう言われても");
        let shared = s.score(&req).unwrap();
        let alone = s.score_each_alone(&req).unwrap();
        assert_eq!(shared.log_probs.len(), CANDIDATES.len());
        for (a, b) in shared.log_probs.iter().zip(&alone) {
            assert!((a - b).abs() < 1e-2, "{format:?}: shared {a} vs alone {b}");
        }
    }
}

#[test]
fn output_is_a_distribution_with_a_margin() {
    let mut s = scorer(PromptFormat::ZenzV25);
    let scores = s.score(&request("htsthqtun", "")).unwrap();
    let sum: f32 = scores.probs.iter().sum();
    assert!((sum - 1.0).abs() < 1e-4, "sum {sum}");
    let mut sorted = scores.probs.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert!((scores.margin - (sorted[0] - sorted[1])).abs() < 1e-6);
    assert!(scores.margin >= 0.0);
}

#[test]
fn growing_input_only_evaluates_the_difference() {
    // aiokey 形式ではスケルトンがプロンプトにあるので、1 文字伸びても前回の先頭部分を使い回せる
    let mut warm = scorer(PromptFormat::Aiokey);
    warm.score(&request("htsthqtu", "そう言われても")).unwrap();
    let incremental = warm.score(&request("htsthqtun", "そう言われても")).unwrap();

    let mut cold = scorer(PromptFormat::Aiokey);
    let fresh = cold.score(&request("htsthqtun", "そう言われても")).unwrap();

    assert!(incremental.reused_tokens > 0);
    assert_eq!(fresh.reused_tokens, 0);
    // 評価したトークン = 一から評価したときのトークン − 使い回したトークン
    assert_eq!(
        incremental.evaluated_tokens + incremental.reused_tokens,
        fresh.evaluated_tokens
    );
    for (a, b) in incremental.log_probs.iter().zip(&fresh.log_probs) {
        assert!((a - b).abs() < 1e-2);
    }
}

#[test]
fn prompt_format_is_switchable() {
    assert_eq!(
        prompt(PromptFormat::ZenzV25, "前の文", "htst", "はたして"),
        "\u{EE00}ハタシテ\u{EE02}前の文\u{EE01}"
    );
    assert_eq!(
        prompt(PromptFormat::ZenzV25, "", "htst", "はたして"),
        "\u{EE00}ハタシテ\u{EE01}"
    );
    assert_eq!(
        prompt(PromptFormat::Aiokey, "前の文", "htst", "はたして"),
        "\u{EE02}前の文\u{EE00}htst\u{EE01}"
    );
    assert_ne!(scorer(PromptFormat::ZenzV25).config().format, PromptFormat::Aiokey);
}

#[test]
fn stage2_result_is_recorded() {
    let doc = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/eval/stage2-zenz-untrained.md"),
    )
    .unwrap();
    for needle in ["top-1", "段1単体", "K = 32", "2 スレッド", "p50", "p95", "zenz-v2.5-xsmall"] {
        assert!(doc.contains(needle), "{needle}");
    }
}

#[test]
fn odd_candidates_and_a_bad_model_path_do_not_panic() {
    // 候補は段1の出力で、入力しだいで何でも入る。空・NUL・とても長い文字列でも落ちない
    let mut s = scorer(PromptFormat::ZenzV25);
    let long = "あ".repeat(400);
    let req = Request {
        context: "\0",
        input: "k\0",
        candidates: vec![("", ""), ("a\0b", "\0"), (long.as_str(), long.as_str()), ("聞く", "きく")],
    };
    let scores = s.score(&req).unwrap();
    assert_eq!(scores.probs.len(), 4);
    assert!(s.score(&Request { context: "", input: "k", candidates: vec![] }).unwrap().probs.is_empty());
    assert!(Scorer::open(&PathBuf::from("/nonexistent.gguf"), &Config::default()).is_err());
}
