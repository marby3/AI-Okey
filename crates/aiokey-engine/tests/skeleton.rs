//! Issue #2 の受け入れテスト：スケルトン変換器と入力パーサー。

use aiokey_engine::input::{parse, Unit, SEPARATOR};
use aiokey_engine::skeleton::skeletonize;

fn consonant(letter: char) -> Unit {
    Unit::Consonant {
        letter,
        vowel: None,
    }
}

#[test]
fn spec_examples_are_converted() {
    let cases = [
        ("か", "k"),
        ("き", "k"),
        ("く", "k"),
        ("し", "s"),
        ("ち", "t"),
        ("つ", "t"),
        ("ふ", "h"),
        ("じ", "z"),
        ("う", "u"),
        ("ほん", "hq"),
        ("まって", "mxt"),
        ("きょ", "ky"),
        ("ラーメン", "r-mq"),
        ("を", "w"),
        ("はたしてほんとうに", "htsthqtun"),
    ];
    for (kana, expected) in cases {
        assert_eq!(skeletonize(kana), expected, "{kana}");
    }
}

#[test]
fn provisional_rules_are_applied() {
    let cases = [
        ("ぢ", "d"),
        ("づ", "d"),
        ("ファ", "h"),
        ("ティ", "t"),
        ("ヴ", "b"),
        ("ゐ", "w"),
        ("ゑ", "w"),
    ];
    for (kana, expected) in cases {
        assert_eq!(skeletonize(kana), expected, "{kana}");
    }
    // ひらがなとカタカナは同じ結果になる
    assert_eq!(
        skeletonize("はたしてほんとうに"),
        skeletonize("ハタシテホントウニ")
    );
}

#[test]
fn consonant_only_input_is_split_into_units() {
    let paths = parse("htst").paths();
    assert_eq!(
        paths,
        vec![vec![
            consonant('h'),
            consonant('t'),
            consonant('s'),
            consonant('t')
        ]]
    );
}

#[test]
fn consonant_followed_by_vowel_has_both_interpretations() {
    let mut paths = parse("ka").paths();
    paths.sort_by_key(|p| p.len());
    assert_eq!(
        paths,
        vec![
            vec![Unit::Consonant {
                letter: 'k',
                vowel: Some('a')
            }],
            vec![consonant('k'), Unit::Vowel('a')],
        ]
    );
}

#[test]
fn slash_marks_an_english_word() {
    assert_eq!(
        parse("/keyboard").paths(),
        vec![vec![Unit::English("keyboard".to_string())]]
    );
}

#[test]
fn shift_space_becomes_a_separator() {
    let input = format!("ht{SEPARATOR}st");
    assert_eq!(
        parse(&input).paths(),
        vec![vec![
            consonant('h'),
            consonant('t'),
            Unit::Separator,
            consonant('s'),
            consonant('t'),
        ]]
    );
}

#[test]
fn unexpected_characters_do_not_break_parsing() {
    // IME から届く入力は信頼できない。想定外の文字は Other として残し、落ちない
    let paths = parse("k\u{0}漢🙂");
    assert_eq!(
        paths.paths(),
        vec![vec![
            consonant('k'),
            Unit::Other('\u{0}'),
            Unit::Other('漢'),
            Unit::Other('🙂'),
        ]]
    );
    assert!(parse("").paths().iter().all(|p| p.is_empty()));
}
