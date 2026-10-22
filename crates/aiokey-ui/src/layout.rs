//! 候補ウィンドウのレイアウト（Mozc 風）。

/// 1 ページの候補の数
pub const PAGE_SIZE: usize = 9;

/// 寸法（論理ピクセル）
#[derive(Debug, Clone)]
pub struct Metrics {
    pub padding: f32,
    pub row_height: f32,
    pub column_gap: f32,
    pub footer_height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub number: String,
    pub text: String,
    pub annotation: String,
    pub selected: bool,
    pub top: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub rows: Vec<Row>,
    pub footer: String,
    pub number_x: f32,
    pub text_x: f32,
    pub annotation_x: f32,
    pub footer_top: f32,
    pub width: f32,
    pub height: f32,
}

/// 候補（表記, 注釈）を並べる。`measure` は文字列の描画幅を返す。
pub fn layout(
    _candidates: &[(String, String)],
    _selection: usize,
    _metrics: &Metrics,
    _measure: impl Fn(&str) -> f32,
) -> Layout {
    todo!()
}
