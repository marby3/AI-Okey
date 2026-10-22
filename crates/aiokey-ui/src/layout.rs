//! 候補ウィンドウのレイアウト（Mozc 風）。

/// 1 ページの候補の数（数字キーで選ぶ範囲と同じにするため、aiokey-keymap の値を使う）
pub use aiokey_keymap::PAGE_SIZE;

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
///
/// 選んでいる候補のあるページ（[`PAGE_SIZE`] 件ずつ）を出し、番号はページごとに 1 から振る。
/// 列の幅は全候補で決めるので、ページを送ってもウィンドウの幅は変わらない。
pub fn layout(
    candidates: &[(String, String)],
    selection: usize,
    metrics: &Metrics,
    measure: impl Fn(&str) -> f32,
) -> Layout {
    let m = metrics;
    let total = candidates.len();
    let selection = selection.min(total.saturating_sub(1));
    let start = selection / PAGE_SIZE * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);

    let widest = |f: &dyn Fn(&(String, String)) -> &str| {
        candidates.iter().map(|c| measure(f(c))).fold(0.0, f32::max)
    };
    let number_x = m.padding;
    let text_x = number_x + measure("9") + m.column_gap;
    let annotation_x = text_x + widest(&|c| &c.0) + m.column_gap;
    let footer = if total == 0 {
        String::new()
    } else {
        format!("{}/{}", selection + 1, total)
    };
    let width =
        (annotation_x + widest(&|c| &c.1) + m.padding).max(m.padding * 2.0 + measure(&footer));

    let rows: Vec<Row> = (start..end)
        .map(|i| Row {
            number: (i - start + 1).to_string(),
            text: candidates[i].0.clone(),
            annotation: candidates[i].1.clone(),
            selected: i == selection,
            top: m.padding + (i - start) as f32 * m.row_height,
        })
        .collect();
    let footer_top = m.padding + rows.len() as f32 * m.row_height;
    Layout {
        rows,
        footer,
        number_x,
        text_x,
        annotation_x,
        footer_top,
        width,
        height: footer_top + m.footer_height + m.padding,
    }
}
