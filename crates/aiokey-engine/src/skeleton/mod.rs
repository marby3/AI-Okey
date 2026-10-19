//! 読み（かな）を、入力規則に従ってスケルトン（子音の列）に変える。
//!
//! 例：はたしてほんとうに → `htsthqtun`

mod rules;

use rules::Mapping;

/// 読みをスケルトンに変える。規則表にない文字（英字・数字・記号など）はそのまま残す。
pub fn skeletonize(reading: &str) -> String {
    let mut out = String::with_capacity(reading.len());
    for c in reading.chars() {
        match rules::lookup(c) {
            Some(Mapping::Letter(letter)) => out.push(letter),
            Some(Mapping::Drop) => {}
            None => out.push(c),
        }
    }
    out
}

/// 読みを、スケルトンの 1 文字とそのかなの母音の組に分ける。並びと長さは [`skeletonize`] の結果と同じ。
///
/// 小書きの母音（ァ など、スケルトンには出ない）は、前の文字の母音を置き換える（ファ → (h, a)）。
/// 混在入力で打った母音と照らし合わせるのに使う。
pub fn units(reading: &str) -> Vec<(char, Option<char>)> {
    let mut out: Vec<(char, Option<char>)> = Vec::with_capacity(reading.len() / 3);
    for c in reading.chars() {
        match rules::lookup(c) {
            Some(Mapping::Letter(letter)) => out.push((letter, rules::vowel_of(c))),
            Some(Mapping::Drop) => {
                if let Some(last) = out.last_mut() {
                    last.1 = rules::vowel_of(c);
                }
            }
            None => out.push((c, None)),
        }
    }
    out
}
