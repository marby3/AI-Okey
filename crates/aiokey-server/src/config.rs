//! サーバーの設定（TOML）。
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Normal,
    BelowNormal,
    Idle,
}

#[derive(Debug, Clone)]
pub struct Stage2Config {
    pub model: Option<PathBuf>,
    pub format: String,
    pub threads: i32,
    pub max_candidates: usize,
    pub temperature: f32,
    pub debounce: Duration,
    pub skip_cost_gap: i32,
    pub unload_after: Duration,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub dictionary: Option<PathBuf>,
    pub k: usize,
    pub priority: Priority,
    pub stage2: Stage2Config,
}

impl Default for Config {
    fn default() -> Self {
        todo!()
    }
}

impl Config {
    pub fn from_toml(_text: &str) -> Result<Self, String> {
        todo!()
    }

    pub fn load(_path: &Path) -> (Self, Option<String>) {
        todo!()
    }
}
