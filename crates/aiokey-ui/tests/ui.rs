//! Issue #12 の受け入れテスト（基準 1〜4）：ネイティブの候補ウィンドウとモード表示。
//!
//! 基準 5・6（ウィンドウを開く時機、数字キー）は aiokey-keymap の tests/window.rs で確かめる。
//! 実際の描画（Direct2D と DirectWrite）は #14 の実機確認で確かめる。

use std::time::{Duration, Instant};

use aiokey_ui::layout::{layout, Metrics, PAGE_SIZE};
use aiokey_ui::mode_indicator::{ModeIndicator, SHOW_FOR};

fn metrics() -> Metrics {
    Metrics {
        padding: 4.0,
        row_height: 24.0,
        column_gap: 8.0,
        footer_height: 20.0,
    }
}

/// 1 文字 10 ピクセルとして測る
fn measure(s: &str) -> f32 {
    s.chars().count() as f32 * 10.0
}

fn candidates(n: usize) -> Vec<(String, String)> {
    (1..=n)
        .map(|i| {
            (
                format!("候補{i}"),
                if i == 2 { "英".into() } else { String::new() },
            )
        })
        .collect()
}

#[test]
fn ui_does_not_depend_on_webview() {
    let lock =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
    for name in ["wry", "tao", "webview2-com"] {
        assert!(
            !lock.contains(&format!("name = \"{name}\"\n")),
            "{name} is still in Cargo.lock"
        );
    }
}

#[test]
fn layout_has_number_candidate_and_annotation_columns_and_a_counter() {
    let m = metrics();
    let l = layout(&candidates(5), 0, &m, measure);
    assert_eq!(l.rows.len(), 5);
    let numbers: Vec<&str> = l.rows.iter().map(|r| r.number.as_str()).collect();
    assert_eq!(numbers, ["1", "2", "3", "4", "5"]);
    assert_eq!(l.rows[1].annotation, "英");
    // 列は 番号 → 候補 → 注釈 の順で、候補の列はいちばん長い候補が収まる幅
    assert!(l.number_x < l.text_x);
    assert!(l.annotation_x >= l.text_x + measure("候補5") + m.column_gap);
    assert!(l.width >= l.annotation_x + measure("英") + m.padding);
    // 行は上から順に並び、下に n/m（選んでいる位置／候補の数）
    assert!(l
        .rows
        .windows(2)
        .all(|w| w[1].top - w[0].top == m.row_height));
    assert_eq!(l.footer, "1/5");
    assert!(l.footer_top >= l.rows.last().unwrap().top + m.row_height);
    assert!(l.height >= l.footer_top + m.footer_height);
    // 1 ページは 9 件
    assert_eq!(PAGE_SIZE, 9);
    assert_eq!(layout(&candidates(20), 0, &m, measure).rows.len(), 9);
}

#[test]
fn selected_row_is_highlighted_and_later_candidates_are_paged() {
    let l = layout(&candidates(20), 10, &metrics(), measure);
    // 11 番目を選んでいると、2 ページ目（10〜18 番目）を出す。番号は 1 から振り直す
    let texts: Vec<&str> = l.rows.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(
        texts,
        (10..=18).map(|i| format!("候補{i}")).collect::<Vec<_>>()
    );
    assert_eq!(l.rows[0].number, "1");
    let selected: Vec<usize> = (0..l.rows.len()).filter(|&i| l.rows[i].selected).collect();
    assert_eq!(selected, [1]);
    assert_eq!(l.footer, "11/20");
    // 最後のページは残りの件数だけ
    let last = layout(&candidates(20), 19, &metrics(), measure);
    assert_eq!(last.rows.len(), 2);
    assert!(last.rows[1].selected);
}

#[test]
fn mode_indicator_shows_for_a_while_after_switching() {
    let t0 = Instant::now();
    let mut indicator = ModeIndicator::default();
    assert_eq!(indicator.visible(t0), None);
    indicator.set_mode("あ", t0);
    assert_eq!(indicator.visible(t0), Some("あ"));
    assert_eq!(
        indicator.visible(t0 + SHOW_FOR - Duration::from_millis(1)),
        Some("あ")
    );
    assert_eq!(indicator.visible(t0 + SHOW_FOR), None);
    // 切り替え直すと、そこから数え直す
    let t1 = t0 + SHOW_FOR / 2;
    indicator.set_mode("A", t1);
    assert_eq!(indicator.visible(t0 + SHOW_FOR), Some("A"));
    assert_eq!(indicator.visible(t1 + SHOW_FOR), None);
}
