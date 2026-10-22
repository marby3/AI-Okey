//! 入力モードの表示（あ／A）を一定時間だけ出す。
use std::time::{Duration, Instant};

/// モード表示を出しておく時間
pub const SHOW_FOR: Duration = Duration::from_millis(1000);

#[derive(Debug, Default)]
pub struct ModeIndicator {}

impl ModeIndicator {
    pub fn set_mode(&mut self, _mode: &str, _now: Instant) {
        todo!()
    }

    /// 今出しておく文字。出さないなら None
    pub fn visible(&self, _now: Instant) -> Option<&str> {
        todo!()
    }
}
