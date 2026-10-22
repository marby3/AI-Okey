//! ライブ変換の表示を、段2の結果で更新する。
//!
//! 段2はサーバーの裏で、キー入力が止まってから動く。打っている間は UI スレッドのタイマーで
//! `GetComposingText` を見に行き、どう使うかは aiokey-keymap の [`live_update`] で決める。
//! 表示の更新は、キー入力の外なので非同期の編集セッション（TF_ES_READWRITE）で行う。

use std::cell::RefCell;
use std::time::{Duration, Instant};

use aiokey_keymap::{live_update, LiveUpdate, Reply};
use anyhow::Result;
use windows::{
    core::AsImpl,
    Win32::{
        Foundation::HWND,
        UI::{
            TextServices::ITfTextInputProcessor,
            WindowsAndMessaging::{KillTimer, SetTimer},
        },
    },
};

use crate::engine::{composition::CompositionState, state::IMEState};

use super::factory::TextServiceFactory;

/// 見に行く間隔
const POLL: Duration = Duration::from_millis(50);
/// これだけ待っても段2が済まなければやめる
const GIVE_UP: Duration = Duration::from_secs(3);

struct Polling {
    tip: ITfTextInputProcessor,
    timer: usize,
    deadline: Instant,
}

thread_local! {
    static POLLING: RefCell<Option<Polling>> = const { RefCell::new(None) };
}

/// 見に行くのを（やり直して）始める。キー入力のたびに呼ぶ。
pub fn start(tip: ITfTextInputProcessor) {
    stop();
    // SAFETY: スレッドのタイマー（ウィンドウなし）。コールバックはこのスレッドのメッセージループで呼ばれる
    let timer = unsafe { SetTimer(HWND::default(), 0, POLL.as_millis() as u32, Some(on_timer)) };
    if timer == 0 {
        tracing::warn!("SetTimer failed");
        return;
    }
    POLLING.with(|p| {
        *p.borrow_mut() = Some(Polling {
            tip,
            timer,
            deadline: Instant::now() + GIVE_UP,
        })
    });
}

/// 見に行くのをやめる。
pub fn stop() {
    if let Some(polling) = POLLING.with(|p| p.borrow_mut().take()) {
        // SAFETY: start で作ったスレッドのタイマーを止める
        let _ = unsafe { KillTimer(HWND::default(), polling.timer) };
    }
}

unsafe extern "system" fn on_timer(_: HWND, _: u32, _: usize, _: u32) {
    let Some((tip, deadline)) =
        POLLING.with(|p| p.borrow().as_ref().map(|p| (p.tip.clone(), p.deadline)))
    else {
        return;
    };
    // SAFETY: tip は TextServiceFactory が実装しているインターフェース
    let factory: &TextServiceFactory = unsafe { tip.as_impl() };
    match factory.poll_live_update() {
        Ok(true) if Instant::now() < deadline => {}
        Ok(_) => stop(),
        Err(e) => {
            tracing::warn!("live update failed: {e:?}");
            stop();
        }
    }
}

impl TextServiceFactory {
    /// 段2の結果を見て、必要なら表示を更新する。まだ待つなら true。
    fn poll_live_update(&self) -> Result<bool> {
        let composition = self.borrow()?.borrow_composition()?.clone();
        if composition.state != CompositionState::Composing {
            return Ok(false);
        }
        let Some(mut ipc_service) = IMEState::get()?.ipc_service.clone() else {
            return Ok(false);
        };
        let candidates = ipc_service.get_composing_text(0)?;
        let reply = Reply {
            revision: candidates.revision,
            settled: candidates.settled,
            reranked: candidates.reranked,
            uncertain: candidates.uncertain,
        };
        match live_update(composition.candidates.revision, &reply) {
            LiveUpdate::Wait => return Ok(true),
            LiveUpdate::Keep => {}
            LiveUpdate::Apply => {
                let text = candidates.texts.first().cloned().unwrap_or_default();
                let sub_text = candidates.sub_texts.first().cloned().unwrap_or_default();
                self.set_text(&text, &sub_text)?;
                ipc_service.set_candidates(candidates.texts.clone())?;
                ipc_service.set_selection(0)?;
                let text_service = self.borrow()?;
                let mut c = text_service.borrow_mut_composition()?;
                c.preview = text;
                c.suffix = sub_text;
                c.raw_hiragana = candidates.hiragana.clone();
                c.corresponding_count =
                    candidates.corresponding_count.first().cloned().unwrap_or(0);
                c.selection_index = 0;
                c.candidates = candidates;
            }
            LiveUpdate::OpenCandidates => {
                // 表示はそのままにして、並べ替えた候補を候補ウィンドウに出す（今の表示の候補を選んでおく）
                let index = candidates
                    .texts
                    .iter()
                    .position(|t| *t == composition.preview)
                    .unwrap_or(0) as i32;
                ipc_service.set_candidates(candidates.texts.clone())?;
                ipc_service.set_selection(index)?;
                ipc_service.show_window()?;
                let text_service = self.borrow()?;
                let mut c = text_service.borrow_mut_composition()?;
                c.selection_index = index;
                c.candidates = candidates;
                c.window_open = true;
            }
        }
        Ok(false)
    }
}
