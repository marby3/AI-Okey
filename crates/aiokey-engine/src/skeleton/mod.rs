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
