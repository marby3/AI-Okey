//! サーバーの設定（TOML）。
//!
//! ```toml
//! dictionary = "aiokey.dic"        # 省略すると実行ファイルと同じフォルダの aiokey.dic
//! k = 32                           # 段1の候補の数
//! priority = "below_normal"        # normal / below_normal / idle
//!
//! [stage2]
//! model = "aiokey-stage2-q8_0.gguf" # 省略すると、実行ファイルの隣の学習済みモデル（なければ段2を使わない）
//! format = "aiokey"                # aiokey / zenz
//! threads = 2                      # 推論のスレッド数
//! max_candidates = 8               # 段2で採点する候補の上限（段1の上位から）
//! # temperature = 3.4             # 省略すると、学習済みモデルの隣の aiokey-stage2.json の値（なければ 1）
//! debounce_ms = 30                 # キー入力がこの時間止まったら段2を動かす
//! skip_cost_gap = 500              # 段1の 1 位と 2 位のコスト差がこれ以上なら段2を呼ばない
//! unload_after_secs = 600          # この時間使わなければ段2のモデルを解放する
//! margin_threshold = 0.1           # 段2の 1 位と 2 位の確率の差がこれ未満なら、確信が低いと知らせる
//! ```
//!
//! 相対パスは設定ファイルのあるフォルダから数える。数値はどれも暫定の既定値で、計測して見直す。
//! 段2の 3 つの閾値（max_candidates、skip_cost_gap、margin_threshold）は、学習済みモデル（stage2-model-v1）で
//! dev セットを使って決めた（docs/eval/stage2-trained.md）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Normal,
    BelowNormal,
    Idle,
}

#[derive(Debug, Clone)]
pub struct Stage2Config {
    /// 段2の GGUF。None なら段2を使わない
    pub model: Option<PathBuf>,
    pub format: String,
    pub threads: i32,
    pub max_candidates: usize,
    /// softmax の温度。None なら学習済みモデルの校正値（[`Config::with_bundled_model`]）、それもなければ 1
    pub temperature: Option<f32>,
    /// キー入力がこの時間止まったら段2を動かす
    pub debounce: Duration,
    /// 段1の 1 位と 2 位のコスト差がこれ以上なら、段2を呼ばない
    pub skip_cost_gap: i32,
    /// この時間使わなければ、段2のモデルを解放する
    pub unload_after: Duration,
    /// 段2の margin（1 位と 2 位の確率の差）がこれ未満なら、確信が低いとして知らせる
    pub margin_threshold: f32,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// 辞書。None なら実行ファイルと同じフォルダの aiokey.dic
    pub dictionary: Option<PathBuf>,
    /// 段1の候補の数
    pub k: usize,
    pub priority: Priority,
    pub stage2: Stage2Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dictionary: None,
            k: 32,
            priority: Priority::BelowNormal,
            stage2: Stage2Config {
                model: None,
                format: "aiokey".into(),
                threads: 2,
                max_candidates: 8,
                temperature: None,
                debounce: Duration::from_millis(30),
                skip_cost_gap: 500,
                unload_after: Duration::from_secs(600),
                margin_threshold: 0.1,
            },
        }
    }
}

/// TOML の形。書かれていない項目は既定値のまま。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    dictionary: Option<PathBuf>,
    k: Option<usize>,
    priority: Option<Priority>,
    stage2: Option<Stage2File>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stage2File {
    model: Option<PathBuf>,
    format: Option<String>,
    threads: Option<i32>,
    max_candidates: Option<usize>,
    temperature: Option<f32>,
    debounce_ms: Option<u64>,
    skip_cost_gap: Option<i32>,
    unload_after_secs: Option<f64>,
    margin_threshold: Option<f32>,
}

/// 配布物に入れる学習済みの段2と、その校正値（温度など）のファイル名
pub const BUNDLED_MODEL: &str = "aiokey-stage2-q8_0.gguf";
pub const BUNDLED_MODEL_INFO: &str = "aiokey-stage2.json";

impl Config {
    /// 設定で段2のモデルを指定していなければ、`dir`（実行ファイルのフォルダ）の学習済みモデルを使う。
    /// 温度を指定していなければ、モデルの隣の [`BUNDLED_MODEL_INFO`] の校正値を使う。
    pub fn with_bundled_model(mut self, dir: &Path) -> Self {
        let model = dir.join(BUNDLED_MODEL);
        if self.stage2.model.is_none() && model.is_file() {
            self.stage2.model = Some(model);
            if self.stage2.temperature.is_none() {
                let info = dir.join(BUNDLED_MODEL_INFO);
                self.stage2.temperature = std::fs::read_to_string(&info)
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                    .and_then(|info| info["temperature"].as_f64())
                    .map(|t| t as f32);
                if self.stage2.temperature.is_none() {
                    // 校正していない温度（1）で動くと確率がずれるので、気づけるように知らせる
                    eprintln!(
                        "aiokey-server: {}: no calibrated temperature; using 1.0",
                        info.display()
                    );
                }
            }
        }
        self
    }

    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut config = Config::default();
        config.dictionary = file.dictionary;
        config.k = file.k.unwrap_or(config.k);
        config.priority = file.priority.unwrap_or(config.priority);
        if let Some(s) = file.stage2 {
            let c = &mut config.stage2;
            c.model = s.model;
            c.format = s.format.unwrap_or(c.format.clone());
            c.threads = s.threads.unwrap_or(c.threads);
            c.max_candidates = s.max_candidates.unwrap_or(c.max_candidates);
            c.temperature = s.temperature.or(c.temperature);
            if let Some(ms) = s.debounce_ms {
                c.debounce = Duration::from_millis(ms);
            }
            c.skip_cost_gap = s.skip_cost_gap.unwrap_or(c.skip_cost_gap);
            c.margin_threshold = s.margin_threshold.unwrap_or(c.margin_threshold);
            if let Some(secs) = s.unload_after_secs {
                c.unload_after = Duration::try_from_secs_f64(secs)
                    .map_err(|e| format!("unload_after_secs: {e}"))?;
            }
        }
        if config.k == 0 || !matches!(config.stage2.format.as_str(), "aiokey" | "zenz") {
            return Err("k must be at least 1, and stage2.format must be aiokey or zenz".into());
        }
        Ok(config)
    }

    /// 設定ファイルを読む。ファイルがなければ既定値。読めない・正しくないときは、IME が動かなくならないよう
    /// 既定値で続け、警告の文を返す。相対パスは設定ファイルのあるフォルダから数える。
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Config::default(), None),
            Err(e) => return (Config::default(), Some(format!("{}: {e}", path.display()))),
        };
        match Config::from_toml(&text) {
            Ok(mut config) => {
                let base = path.parent().unwrap_or(Path::new("."));
                config.dictionary = config.dictionary.map(|p| base.join(p));
                config.stage2.model = config.stage2.model.map(|p| base.join(p));
                (config, None)
            }
            Err(e) => (
                Config::default(),
                Some(format!("{}: {e} (using the defaults)", path.display())),
            ),
        }
    }
}
