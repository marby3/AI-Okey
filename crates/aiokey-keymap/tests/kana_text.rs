//! Issue #14 rc.4 基準 9：かなモードの TSF がサーバーに送る文字。
//!
//! rc.3 までは `/` を `・`、`-` を `ー` に変えて送っていたので、サーバーは英語の指定と長音を読めなかった。

use aiokey_keymap::full_width::kana_mode_text;

#[test]
fn input_rule_characters_reach_the_server_unchanged() {
    // 英語の指定・長音・区切りは、入力の規則に使うのでそのまま送る
    assert_eq!(kana_mode_text("/hqt"), "/hqt");
    assert_eq!(kana_mode_text("g-dr"), "g-dr");
    assert_eq!(kana_mode_text(" "), " ");
    // そのほかの記号は、これまでどおり全角にする
    assert_eq!(kana_mode_text(","), "、");
    assert_eq!(kana_mode_text("."), "。");
}
