//! Issue #5 の受け入れテスト：英語混在入力。
//!
//! Mozc のデータ（`scripts/fetch-mozc.sh` で取得）と、ビルダーに同梱した英単語の頻度 TSV から辞書を作る。

use std::path::PathBuf;
use std::sync::OnceLock;

use aiokey_engine::convert::{convert, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_engine::skeleton::skeletonize;

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

fn surfaces(input: &str) -> Vec<String> {
    convert(dictionary(), input, &Options::default())
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

#[test]
fn english_words_come_from_the_frequency_tsv() {
    // 辞書に、つづりを見出しにした英単語が入っている
    let entries = dictionary().lookup("keyboard");
    assert!(
        entries.iter().any(|e| e.surface == "keyboard"),
        "{entries:?}"
    );
    let s = surfaces("keyboard");
    assert!(s.iter().any(|x| x == "keyboard"), "{s:?}");
}

#[test]
fn skeleton_interpretation_stays_next_to_the_english_word() {
    let s = surfaces("queue");
    assert!(s.iter().any(|x| x == "queue"), "{s:?}");
    // かなを含み、スケルトンにすると入力と同じになる候補（スケルトンとしての解釈）
    assert!(
        s.iter().any(|x| !x.is_ascii() && skeletonize(x) == "queue"),
        "{s:?}"
    );
}

// K = 1 では、いちばんよい候補を優先して打った文字列は入れない（#30、tests/k_order.rs）
#[test]
fn raw_input_is_a_candidate_when_k_is_2_or_more() {
    for (input, k) in [
        ("htsthqtun", 32),
        ("htsthqtun", 3),
        ("zzzz", 32),
        ("kik", 2),
    ] {
        let candidates = convert(dictionary(), input, &Options { k });
        assert!(candidates.len() <= k);
        assert!(
            candidates.iter().any(|c| c.surface == input),
            "{input} (k={k}): {:?}",
            candidates.iter().map(|c| &c.surface).collect::<Vec<_>>()
        );
    }
}

#[test]
fn slash_forces_english_only() {
    assert_eq!(surfaces("/hqt"), vec!["hqt".to_string()]);
}

#[test]
fn english_word_mixes_with_japanese() {
    let s = surfaces("knkeyboardds");
    assert!(s.iter().any(|x| x == "このkeyboardです"), "{s:?}");
}

#[test]
fn malformed_frequency_tsv_is_rejected() {
    // 頻度 TSV はコーパスから作る外部の入力。形が崩れていればエラーにする（黙って読み飛ばさない）
    let dir = std::env::temp_dir().join(format!("aiokey-english-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in [
        ("no_tab.tsv", "keyboard 100\n"),
        ("bad_freq.tsv", "keyboard\tlots\n"),
        ("empty_word.tsv", "\t100\n"),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        assert!(aiokey_dictbuild::read_english_tsv(&path).is_err(), "{name}");
    }
}

// #14 rc.4 基準 9：`/` のあとの英字はそのまま、`/` だけなら「・」
#[test]
fn slash_marks_english_and_a_lone_slash_is_a_middle_dot() {
    let s = surfaces("/hqt");
    assert_eq!(s.first().map(String::as_str), Some("hqt"), "{s:?}");
    let s = surfaces("/");
    assert_eq!(s.first().map(String::as_str), Some("・"), "{s:?}");
}

// #47 基準 1：文の途中の `/` は英語の指定とも「・」とも読み、どちらの候補も上位 5 件に入る。
// 「ジョン・F・ケネディ」の順位は記録のために出す（cargo test -- --nocapture）
#[test]
fn english_and_middle_dot_candidates_are_both_near_the_top() {
    for (input, word) in [
        ("zyq/F/knd", "F"),
        ("htst/hqt", "hqt"),
        ("htst/keyboard", "keyboard"),
    ] {
        let s = surfaces(input);
        let top: Vec<_> = s.iter().take(5).collect();
        assert!(
            top.iter().any(|x| x.contains(word) && !x.contains('・')),
            "{input}: {top:?}"
        );
        assert!(top.iter().any(|x| x.contains('・')), "{input}: {top:?}");
    }
    let s = convert(dictionary(), "zyq/F/knd", &Options { k: 3000 });
    let rank = s.iter().position(|c| c.surface == "ジョン・F・ケネディ");
    eprintln!(
        "zyq/F/knd: ジョン・F・ケネディ は {:?} 位（1 始まり、K=3000）",
        rank.map(|r| r + 1)
    );
}

// #52 基準 1：`/` のあとが長くても、英語の指定と「・」の候補がどちらも上位 5 件に入り、1 位は英語の指定
#[test]
fn middle_dot_candidates_stay_near_the_top_after_a_long_tail() {
    for (input, word) in [
        ("prnxt/niqth、", "niqth"),
        ("aiqsytiq/rqgtybrms。", "rqgtybrms"),
        ("zyq/F/knd", "F"),
        ("htst/hqt", "hqt"),
        ("htst/keyboard", "keyboard"),
    ] {
        let s = surfaces(input);
        let top: Vec<_> = s.iter().take(5).collect();
        assert!(
            top[0].contains(word) && !top[0].contains('・'),
            "{input}: {top:?}"
        );
        assert!(top.iter().any(|x| x.contains('・')), "{input}: {top:?}");
    }
}

// #52 基準 3：英語の語を閉じる `/` は、語の終わりとしても読む
#[test]
fn slash_closing_an_english_word_can_just_end_it() {
    let s = surfaces("wtsh/Python/wtkims");
    assert!(s.iter().any(|x| x == "私はPythonを使います"), "{s:?}");
}
