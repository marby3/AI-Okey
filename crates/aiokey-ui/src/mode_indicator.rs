//! 入力モードの表示（あ／A）を一定時間だけ出す。
use std::time::{Duration, Instant};

/// モード表示を出しておく時間
pub const SHOW_FOR: Duration = Duration::from_millis(1000);

#[derive(Debug, Default)]
pub struct ModeIndicator {
    shown: Option<(String, Instant)>,
}

impl ModeIndicator {
    /// モードを切り替えた。`now` から [`SHOW_FOR`] のあいだ出す
    pub fn set_mode(&mut self, mode: &str, now: Instant) {
        self.shown = Some((mode.to_string(), now));
    }

    /// 今出しておく文字。出さないなら None
    pub fn visible(&self, now: Instant) -> Option<&str> {
        match &self.shown {
            Some((mode, since)) if now.saturating_duration_since(*since) < SHOW_FOR => Some(mode),
            _ => None,
        }
    }
}
