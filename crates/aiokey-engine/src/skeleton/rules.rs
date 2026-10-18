//! かな → スケルトンの規則表。
//!
//! 仕様書の入力規則の表に、仕様にないかなの扱い（仮決定）を足したもの。
//! 仮決定の規則は後で変わりうるので、規則はこの表 1 か所にまとめておく。

use std::collections::HashMap;
use std::sync::LazyLock;

/// かな 1 文字の変換先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mapping {
    /// スケルトンの 1 文字に変える
    Letter(char),
    /// 出力しない
    Drop,
}

/// 仕様書の表（ひらがなで書く。カタカナはひらがなに直してから引く）。
const SPEC_RULES: &[(&str, char)] = &[
    // ア行は母音をそのまま
    ("あ", 'a'),
    ("い", 'i'),
    ("う", 'u'),
    ("え", 'e'),
    ("お", 'o'),
    // か行〜ぱ行は行ごとに子音 1 文字（し・ち・つ・ふ・じ も行の子音に揃える）
    ("かきくけこ", 'k'),
    ("がぎぐげご", 'g'),
    ("さしすせそ", 's'),
    ("ざじずぜぞ", 'z'),
    ("たちつてと", 't'),
    ("だでど", 'd'),
    ("なにぬねの", 'n'),
    ("はひふへほ", 'h'),
    ("ばびぶべぼ", 'b'),
    ("ぱぴぷぺぽ", 'p'),
    ("まみむめも", 'm'),
    ("やゆよ", 'y'),
    ("らりるれろ", 'r'),
    // を は わ と共通
    ("わを", 'w'),
    // ん・っ は専用キー、長音は -
    ("ん", 'q'),
    ("っ", 'x'),
    ("ー", '-'),
    // 拗音は 子音 + y
    ("ゃゅょ", 'y'),
];

/// 仕様書の表にないかなの扱い（仮決定）。
const PROVISIONAL_RULES: &[(&str, char)] = &[
    ("ぢづ", 'd'),
    ("ゐゑ", 'w'),
    ("ゔ", 'b'),
    // 小書きの ゕ・ゖ（「ヵ月」など）は か行
    ("ゕゖ", 'k'),
];

/// 出力しないかな（仮決定）。ファ → h、ティ → t のように、前の子音だけを残す。
const PROVISIONAL_DROPS: &str = "ぁぃぅぇぉゎ";

static TABLE: LazyLock<HashMap<char, Mapping>> = LazyLock::new(|| {
    let mut table = HashMap::new();
    for (kana, letter) in SPEC_RULES.iter().chain(PROVISIONAL_RULES) {
        for c in kana.chars() {
            table.insert(c, Mapping::Letter(*letter));
        }
    }
    for c in PROVISIONAL_DROPS.chars() {
        table.insert(c, Mapping::Drop);
    }
    table
});

/// かな 1 文字の変換先。表にない文字は `None`。
pub(crate) fn lookup(kana: char) -> Option<Mapping> {
    TABLE.get(&to_hiragana(kana)).copied()
}

/// カタカナ（ァ〜ヶ）をひらがなに直す。長音記号 ー はそのまま。
fn to_hiragana(c: char) -> char {
    match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}
