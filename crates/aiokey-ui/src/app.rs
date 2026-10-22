//! 候補ウィンドウとモード表示（Win32、Direct2D、DirectWrite）。
//!
//! gRPC（名前付きパイプ）で受けた操作を、UI スレッドのウィンドウへ PostMessage で渡し、
//! そのスレッドで描画する。レイアウトとモード表示の時間は OS に依存しない [`crate::layout`] と
//! [`crate::mode_indicator`] に任せる。

use std::cell::RefCell;
use std::time::{Duration, Instant};

use aiokey_ipc::proto::window_service_server::WindowServiceServer;
use aiokey_server::TonicNamedPipeServer;
use aiokey_ui::layout::{layout, Layout, Metrics};
use aiokey_ui::mode_indicator::ModeIndicator;
use anyhow::{Context as _, Result};
use tonic::transport::Server;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
                    D2D_SIZE_U,
                },
                D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
                D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES,
                D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
            },
            DirectWrite::{
                DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
                DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
            },
            Dxgi::Common::DXGI_FORMAT_UNKNOWN,
            Gdi::{
                BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromRect,
                MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{
                GetDpiForWindow, SetProcessDpiAwarenessContext,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer,
                PostMessageW, RegisterClassW, SetTimer, SetWindowPos, ShowWindow, TranslateMessage,
                CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SW_HIDE,
                SW_SHOWNOACTIVATE, WM_APP, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
};

use super::ipc::{WindowAction, WindowController, WindowService};
use super::uiaccess::prepare_uiaccess_token;

/// gRPC から UI スレッドへ操作を渡すメッセージ（LPARAM は Box<WindowAction>）
const WM_ACTION: u32 = WM_APP + 1;
/// モード表示を消すかどうかを確かめるタイマー
const INDICATOR_TIMER: usize = 1;
const INDICATOR_TICK: Duration = Duration::from_millis(100);
/// モード表示の大きさ（DIP）
const INDICATOR_SIZE: f32 = 44.0;

/// 寸法（DIP）。見た目は Mozc に寄せる
fn metrics() -> Metrics {
    Metrics {
        padding: 6.0,
        row_height: 26.0,
        column_gap: 10.0,
        footer_height: 20.0,
    }
}

fn rgb(hex: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

struct Ui {
    candidate: HWND,
    indicator: HWND,
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    text_format: IDWriteTextFormat,
    small_format: IDWriteTextFormat,
    footer_format: IDWriteTextFormat,
    indicator_format: IDWriteTextFormat,
    candidate_target: Option<ID2D1HwndRenderTarget>,
    indicator_target: Option<ID2D1HwndRenderTarget>,
    candidates: Vec<(String, String)>,
    selection: usize,
    layout: Option<Layout>,
    /// キャレットの位置（物理ピクセル）
    caret: RECT,
    mode: ModeIndicator,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

pub fn run() -> Result<()> {
    prepare_uiaccess_token()?;
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        lpszClassName: w!("AiokeyUiWindow"),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        anyhow::bail!("RegisterClassW failed");
    }
    let create = |title: PCWSTR| unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            w!("AiokeyUiWindow"),
            title,
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )
    };
    let candidate = create(w!("CandidateList")).context("CreateWindowExW (candidate)")?;
    let indicator = create(w!("Indicator")).context("CreateWindowExW (indicator)")?;

    let d2d: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
    let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
    let format = |size: f32, bold: bool| -> Result<IDWriteTextFormat> {
        let weight = if bold {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        Ok(unsafe {
            dwrite.CreateTextFormat(
                w!("Yu Gothic UI"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("ja-jp"),
            )?
        })
    };
    let text_format = format(15.0, false)?;
    let small_format = format(12.0, false)?;
    let footer_format = format(11.0, false)?;
    unsafe { footer_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)? };
    let indicator_format = format(24.0, true)?;
    unsafe {
        indicator_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        indicator_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    }
    UI.with(|ui| {
        *ui.borrow_mut() = Some(Ui {
            candidate,
            indicator,
            d2d,
            dwrite,
            text_format,
            small_format,
            footer_format,
            indicator_format,
            candidate_target: None,
            indicator_target: None,
            candidates: Vec::new(),
            selection: 0,
            layout: None,
            caret: RECT::default(),
            mode: ModeIndicator::default(),
        })
    });

    // gRPC は別スレッドの tokio で受け、UI スレッドのウィンドウへ渡す（HWND は Send でないので値で渡す）
    let target = candidate.0 as isize;
    std::thread::spawn(move || serve(target));

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn serve(target: isize) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<WindowAction>(32);
    tokio::spawn(async move {
        while let Some(action) = rx.recv().await {
            let action = Box::into_raw(Box::new(action));
            let hwnd = HWND(target as *mut std::ffi::c_void);
            // SAFETY: 受け取った側（wndproc の WM_ACTION）が Box に戻して捨てる
            if unsafe { PostMessageW(hwnd, WM_ACTION, WPARAM(0), LPARAM(action as isize)) }.is_err()
            {
                drop(unsafe { Box::from_raw(action) });
            }
        }
    });
    let service = WindowService {
        controller: WindowController::new(tx),
    };
    println!("WindowServer listening");
    if let Err(e) = Server::builder()
        .add_service(WindowServiceServer::new(service))
        .serve_with_incoming(TonicNamedPipeServer::new("aiokey_ui"))
        .await
    {
        eprintln!("aiokey-ui: gRPC server failed: {e}");
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ACTION => {
            // SAFETY: serve が Box::into_raw で渡したもの
            let action = unsafe { Box::from_raw(lparam.0 as *mut WindowAction) };
            with_ui(|ui| ui.apply(*action));
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            with_ui(|ui| ui.paint(hwnd));
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == INDICATOR_TIMER => {
            with_ui(|ui| ui.tick_indicator());
            LRESULT(0)
        }
        // クリックされても前に出ず、入力先のアプリからフォーカスを奪わない
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn with_ui(f: impl FnOnce(&mut Ui) -> Result<()>) {
    UI.with(|ui| {
        if let Some(ui) = ui.borrow_mut().as_mut() {
            if let Err(e) = f(ui) {
                eprintln!("aiokey-ui: {e:?}");
            }
        }
    });
}

impl Ui {
    fn apply(&mut self, action: WindowAction) -> Result<()> {
        match action {
            WindowAction::Show => {
                if !self.candidates.is_empty() {
                    unsafe {
                        let _ = ShowWindow(self.indicator, SW_HIDE);
                        let _ = ShowWindow(self.candidate, SW_SHOWNOACTIVATE);
                    }
                }
            }
            WindowAction::Hide => unsafe {
                let _ = ShowWindow(self.candidate, SW_HIDE);
            },
            WindowAction::SetPosition {
                top,
                left,
                bottom,
                right,
            } => {
                self.caret = RECT {
                    left,
                    top,
                    right,
                    bottom,
                };
                self.relayout()?;
            }
            WindowAction::SetCandidate { candidates } => {
                // 注釈は今のところない（列だけ用意する）
                self.candidates = candidates.into_iter().map(|c| (c, String::new())).collect();
                self.selection = 0;
                self.relayout()?;
            }
            WindowAction::SetSelection { index } => {
                self.selection = index.max(0) as usize;
                self.relayout()?;
            }
            WindowAction::SetInputMode(mode) => {
                self.mode.set_mode(&mode, Instant::now());
                let scale = self.scale(self.indicator);
                let size = (INDICATOR_SIZE * scale) as i32;
                unsafe {
                    SetWindowPos(
                        self.indicator,
                        HWND_TOPMOST,
                        self.caret.left,
                        self.caret.bottom + 4,
                        size,
                        size,
                        SWP_NOACTIVATE,
                    )?;
                    let _ = ShowWindow(self.indicator, SW_SHOWNOACTIVATE);
                    let _ = InvalidateRect(self.indicator, None, false);
                    SetTimer(
                        self.indicator,
                        INDICATOR_TIMER,
                        INDICATOR_TICK.as_millis() as u32,
                        None,
                    );
                }
            }
        }
        Ok(())
    }

    fn tick_indicator(&mut self) -> Result<()> {
        if self.mode.visible(Instant::now()).is_none() {
            unsafe {
                let _ = ShowWindow(self.indicator, SW_HIDE);
                let _ = KillTimer(self.indicator, INDICATOR_TIMER);
            }
        }
        Ok(())
    }

    /// DIP から物理ピクセルへの倍率
    fn scale(&self, hwnd: HWND) -> f32 {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    }

    fn measure(&self, format: &IDWriteTextFormat, text: &str) -> f32 {
        let text = wide(text);
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&text, format, 10_000.0, 1_000.0)
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        match layout.and_then(|l| unsafe { l.GetMetrics(&mut m) }) {
            Ok(()) => m.widthIncludingTrailingWhitespace,
            Err(_) => 0.0,
        }
    }

    /// レイアウトを計算し直し、ウィンドウの大きさと位置を合わせて描き直す。
    fn relayout(&mut self) -> Result<()> {
        if self.candidates.is_empty() {
            self.layout = None;
            unsafe {
                let _ = ShowWindow(self.candidate, SW_HIDE);
            }
            return Ok(());
        }
        let l = layout(&self.candidates, self.selection, &metrics(), |s| {
            self.measure(&self.text_format, s)
        });
        let scale = self.scale(self.candidate);
        let (width, height) = (
            (l.width * scale).ceil() as i32,
            (l.height * scale).ceil() as i32,
        );
        let (x, y) = place(self.caret, width, height);
        unsafe {
            SetWindowPos(
                self.candidate,
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            )?;
        }
        if let Some(target) = &self.candidate_target {
            unsafe {
                target.Resize(&D2D_SIZE_U {
                    width: width as u32,
                    height: height as u32,
                })?
            };
        }
        self.layout = Some(l);
        unsafe {
            let _ = InvalidateRect(self.candidate, None, false);
        }
        Ok(())
    }

    fn target(&mut self, hwnd: HWND) -> Result<ID2D1HwndRenderTarget> {
        let slot = if hwnd == self.candidate {
            &self.candidate_target
        } else {
            &self.indicator_target
        };
        if let Some(t) = slot {
            return Ok(t.clone());
        }
        let mut rect = RECT::default();
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect)? };
        let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_UNKNOWN,
                alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
            },
            dpiX: dpi,
            dpiY: dpi,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U {
                width: (rect.right - rect.left).max(1) as u32,
                height: (rect.bottom - rect.top).max(1) as u32,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let target = unsafe { self.d2d.CreateHwndRenderTarget(&props, &hwnd_props)? };
        if hwnd == self.candidate {
            self.candidate_target = Some(target.clone());
        } else {
            self.indicator_target = Some(target.clone());
        }
        Ok(target)
    }

    fn paint(&mut self, hwnd: HWND) -> Result<()> {
        let target = self.target(hwnd)?;
        let brush = |hex| -> Result<ID2D1SolidColorBrush> {
            Ok(unsafe { target.CreateSolidColorBrush(&rgb(hex), None)? })
        };
        let size = unsafe { target.GetSize() };
        let all = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: size.width,
            bottom: size.height,
        };
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&rgb(0xFFFFFF)));
            target.DrawRectangle(&all, &brush(0xC8C8C8)?, 1.0, None);
        }
        if hwnd == self.indicator {
            if let Some(mode) = self.mode.visible(Instant::now()) {
                unsafe {
                    target.DrawText(
                        &wide(mode),
                        &self.indicator_format,
                        &all,
                        &brush(0x333333)?,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        } else if let Some(l) = &self.layout {
            let m = metrics();
            let (text, sub, highlight) = (brush(0x000000)?, brush(0x707070)?, brush(0xD1EAFF)?);
            for row in &l.rows {
                let rect = |x: f32, right: f32| D2D_RECT_F {
                    left: x,
                    top: row.top,
                    right,
                    bottom: row.top + m.row_height,
                };
                unsafe {
                    if row.selected {
                        target.FillRectangle(&rect(1.0, size.width - 1.0), &highlight);
                    }
                    for (s, format, brush, x, right) in [
                        (&row.number, &self.small_format, &sub, l.number_x, l.text_x),
                        (
                            &row.text,
                            &self.text_format,
                            &text,
                            l.text_x,
                            l.annotation_x,
                        ),
                        (
                            &row.annotation,
                            &self.small_format,
                            &sub,
                            l.annotation_x,
                            size.width,
                        ),
                    ] {
                        // 行の高さの中に、上下の余白をとって置く
                        let r = D2D_RECT_F {
                            top: row.top + 3.0,
                            ..rect(x, right)
                        };
                        target.DrawText(
                            &wide(s),
                            format,
                            &r,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }
            }
            let footer = D2D_RECT_F {
                left: m.padding,
                top: l.footer_top,
                right: size.width - m.padding,
                bottom: l.footer_top + m.footer_height,
            };
            unsafe {
                target.DrawText(
                    &wide(&l.footer),
                    &self.footer_format,
                    &footer,
                    &sub,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
        unsafe { target.EndDraw(None, None)? };
        Ok(())
    }
}

/// キャレットの下に置く。画面の下や右にはみ出すなら、上や左にずらす。
fn place(caret: RECT, width: i32, height: i32) -> (i32, i32) {
    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let handle = MonitorFromRect(&caret, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(handle, &mut monitor);
    }
    let work = monitor.rcWork;
    let y = if caret.bottom + height > work.bottom {
        caret.top - height
    } else {
        caret.bottom
    };
    let x = (caret.left - 15).min(work.right - width).max(work.left);
    (x, y)
}
