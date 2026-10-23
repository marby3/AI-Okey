//! Issue #10 の受け入れテスト：変換サーバー。
//!
//! 辞書は Mozc のデータ（`scripts/fetch-mozc.sh` で取得）から作る。段2はテスト用の採点器に差し替える。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use aiokey_engine::dict::Dictionary;
use aiokey_ipc::proto::{
    aiokey_service_server::AiokeyService, AppendTextRequest, ClearTextRequest, ComposingText,
    ConvertRequest, GetComposingTextRequest, MoveCursorRequest, RemoveTextRequest,
    SetContextRequest, ShrinkTextRequest,
};
use aiokey_server::config::{Config, Priority};
use aiokey_server::service::{ConversionService, Loader, Reranker};
use tonic::Request;

fn mozc_dir() -> PathBuf {
    std::env::var_os("AIOKEY_MOZC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mozc"))
}

fn dictionary() -> Arc<Dictionary> {
    static DICT: OnceLock<Arc<Dictionary>> = OnceLock::new();
    DICT.get_or_init(|| {
        let bytes = aiokey_dictbuild::build_from_mozc(&mozc_dir())
            .expect("辞書を作れない。scripts/fetch-mozc.sh を実行したか確かめる");
        Arc::new(Dictionary::from_bytes(bytes).unwrap())
    })
    .clone()
}

/// テスト用の段2。採点した入力を記録し、段1の並びを逆にする点数を返す。
struct Fake {
    calls: Arc<Mutex<Vec<String>>>,
    delay: Duration,
}

impl Reranker for Fake {
    fn score(
        &mut self,
        _context: &str,
        input: &str,
        candidates: &[(String, String)],
    ) -> Result<Vec<f32>, String> {
        std::thread::sleep(self.delay);
        self.calls.lock().unwrap().push(input.to_string());
        Ok((0..candidates.len()).map(|i| i as f32).collect())
    }
}

struct Harness {
    service: ConversionService,
    calls: Arc<Mutex<Vec<String>>>,
    loads: Arc<AtomicUsize>,
}

fn harness(config: Config, delay: Duration) -> Harness {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let loads = Arc::new(AtomicUsize::new(0));
    let loader: Loader = {
        let (calls, loads) = (calls.clone(), loads.clone());
        Arc::new(move || {
            loads.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Fake {
                calls: calls.clone(),
                delay,
            }) as Box<dyn Reranker>)
        })
    };
    Harness {
        service: ConversionService::new(dictionary(), config, Some(loader)),
        calls,
        loads,
    }
}

/// 段2を必ず呼ぶ設定（コスト差の閾値を最大にする）。debug ビルドの段1でも打鍵の間に段2が動かないよう、
/// 止まったとみなすまでの時間は長めにする。
fn always_stage2() -> Config {
    let mut config = Config::default();
    config.stage2.skip_cost_gap = i32::MAX;
    config.stage2.debounce = Duration::from_millis(200);
    config
}

async fn append(service: &ConversionService, text: &str) -> ComposingText {
    service
        .append_text(Request::new(AppendTextRequest {
            text_to_append: text.to_string(),
        }))
        .await
        .unwrap()
        .into_inner()
        .composing_text
        .unwrap()
}

async fn convert(service: &ConversionService) -> ComposingText {
    service
        .convert(Request::new(ConvertRequest {}))
        .await
        .unwrap()
        .into_inner()
        .composing_text
        .unwrap()
}

async fn latest(service: &ConversionService, wait_ms: u32) -> ComposingText {
    service
        .get_composing_text(Request::new(GetComposingTextRequest { wait_ms }))
        .await
        .unwrap()
        .into_inner()
        .composing_text
        .unwrap()
}

fn texts(c: &ComposingText) -> Vec<String> {
    c.suggestions.iter().map(|s| s.text.clone()).collect()
}

#[tokio::test]
async fn session_operations_convert_character_by_character() {
    let h = harness(Config::default(), Duration::ZERO);
    let s = &h.service;
    s.set_context(Request::new(SetContextRequest {
        context: "それは".into(),
    }))
    .await
    .unwrap();

    let mut last = None;
    for c in "htsthqtun".chars() {
        last = Some(append(s, &c.to_string()).await);
    }
    let last = last.unwrap();
    assert!(
        texts(&last).iter().any(|t| t == "果たして本当に"),
        "{:?}",
        texts(&last)
    );
    assert!(last.suggestions.iter().all(|x| x.corresponding_count == 9));

    // Kana モードの TSF は英字を全角で送ってくる。半角に直して同じように扱う
    let wide = append(s, "ｘ").await;
    assert!(wide.suggestions.iter().all(|x| x.corresponding_count == 10));
    let removed = s
        .remove_text(Request::new(RemoveTextRequest {}))
        .await
        .unwrap()
        .into_inner()
        .composing_text
        .unwrap();
    assert_eq!(texts(&removed), texts(&last));

    // カーソルを 3 文字戻すと、カーソルより前（htsthq ＝ はたしてほん）の候補になる
    let moved = s
        .move_cursor(Request::new(MoveCursorRequest { offset: -3 }))
        .await
        .unwrap()
        .into_inner()
        .composing_text
        .unwrap();
    assert!(moved.suggestions.iter().all(|x| x.corresponding_count == 6));
    assert!(
        texts(&moved).iter().any(|t| t == "果たして本"),
        "{:?}",
        texts(&moved)
    );
    s.move_cursor(Request::new(MoveCursorRequest { offset: 3 }))
        .await
        .unwrap();

    // 先頭の 4 文字（htst ＝ 果たして）を確定して落とすと、残りは hqtun（本当に）
    let shrunk = s
        .shrink_text(Request::new(ShrinkTextRequest { offset: 4 }))
        .await
        .unwrap()
        .into_inner()
        .composing_text
        .unwrap();
    assert!(
        texts(&shrunk).iter().any(|t| t == "本当に"),
        "{:?}",
        texts(&shrunk)
    );
    assert!(shrunk
        .suggestions
        .iter()
        .all(|x| x.corresponding_count == 5));

    s.clear_text(Request::new(ClearTextRequest {}))
        .await
        .unwrap();
    assert!(latest(s, 0).await.suggestions.is_empty());
}

#[tokio::test]
async fn append_text_rejects_empty_input() {
    let h = harness(Config::default(), Duration::ZERO);
    let status = h
        .service
        .append_text(Request::new(AppendTextRequest {
            text_to_append: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(status.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn stage2_runs_only_after_a_pause_or_on_convert_and_drops_stale_requests() {
    let h = harness(always_stage2(), Duration::ZERO);
    let s = &h.service;

    // 続けて打っている間は段2を呼ばない。止まったら、最後の入力だけを採点する
    for c in "kik".chars() {
        let c1 = append(s, &c.to_string()).await;
        assert!(!c1.reranked);
    }
    assert!(h.calls.lock().unwrap().is_empty());
    let settled = latest(s, 1000).await;
    assert_eq!(*h.calls.lock().unwrap(), vec!["kik".to_string()]);
    assert!(settled.reranked);

    // 変換キーなら待たずに採点する
    append(s, "s").await;
    let converted = convert(s).await;
    assert!(converted.reranked);
    assert_eq!(h.calls.lock().unwrap().last().unwrap(), "kiks");

    // 採点の途中で入力が変わったら、その結果は捨てる
    let slow = harness(always_stage2(), Duration::from_millis(300));
    append(&slow.service, "k").await;
    tokio::time::sleep(Duration::from_millis(350)).await; // 止まったとみなされ（200ms）、段2が始まる
    let newer = append(&slow.service, "i").await;
    assert!(!newer.reranked);
    let after = latest(&slow.service, 2000).await;
    assert_eq!(after.revision, newer.revision);
    assert_eq!(
        *slow.calls.lock().unwrap(),
        vec!["k".to_string(), "ki".to_string()]
    );
    assert!(after.reranked);
}

#[tokio::test]
async fn confident_stage1_skips_stage2() {
    // コスト差の閾値が 0 なら、いつでも「確信度が高い」ので段2を呼ばない
    let mut config = Config::default();
    config.stage2.skip_cost_gap = 0;
    let h = harness(config, Duration::ZERO);
    append(&h.service, "htsthqtun").await;
    assert!(!latest(&h.service, 1000).await.reranked);
    assert!(!convert(&h.service).await.reranked);
    assert!(h.calls.lock().unwrap().is_empty());
    assert_eq!(h.loads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn idle_model_is_released_and_reloaded() {
    let mut config = always_stage2();
    config.stage2.unload_after = Duration::from_millis(200);
    let h = harness(config, Duration::ZERO);
    append(&h.service, "kik").await;
    convert(&h.service).await;
    assert!(h.service.stage2_loaded());
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!h.service.stage2_loaded());
    assert_eq!(h.loads.load(Ordering::SeqCst), 1);
    // 次に段2が要るとき（入力が変わって変換キーを押したとき）に読み込み直す
    append(&h.service, "s").await;
    convert(&h.service).await;
    assert!(h.service.stage2_loaded());
    assert_eq!(h.loads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn stage2_reports_the_margin_and_whether_it_is_uncertain() {
    // テスト用の段2は点数を 0, 1, 2, … と返すので、並べ替えたあとの 1 位と 2 位の差は 1
    for (threshold, uncertain) in [(2.0, true), (0.5, false)] {
        let mut config = always_stage2();
        config.stage2.margin_threshold = threshold;
        let h = harness(config, Duration::ZERO);
        append(&h.service, "kik").await;
        let c = convert(&h.service).await;
        assert!(c.reranked && c.settled);
        assert!((c.margin - 1.0).abs() < 1e-6, "{}", c.margin);
        assert_eq!(c.uncertain, uncertain, "threshold {threshold}");
    }
}

#[test]
fn trained_model_next_to_the_server_is_the_default() {
    // Issue #13 基準 9：実行ファイルの隣に学習済みモデルがあれば、それを aiokey 形式と校正後の温度で使う
    let dir = std::env::temp_dir().join(format!("aiokey-bundled-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let defaults = Config::default();
    assert_eq!(defaults.stage2.format, "aiokey");
    assert_eq!(defaults.clone().with_bundled_model(&dir).stage2.model, None);

    std::fs::write(dir.join("aiokey-stage2-q8_0.gguf"), b"gguf").unwrap();
    std::fs::write(dir.join("aiokey-stage2.json"), r#"{"temperature": 3.25}"#).unwrap();
    let config = defaults.with_bundled_model(&dir);
    assert_eq!(config.stage2.model, Some(dir.join("aiokey-stage2-q8_0.gguf")));
    assert_eq!(config.stage2.temperature, Some(3.25));

    // 設定ファイルで指定したものが優先する
    let mut own = Config::from_toml("[stage2]\nmodel = \"other.gguf\"\ntemperature = 1.5").unwrap();
    own = own.with_bundled_model(&dir);
    assert_eq!(own.stage2.model, Some(PathBuf::from("other.gguf")));
    assert_eq!(own.stage2.temperature, Some(1.5));
}

#[test]
fn toml_sets_threads_priority_thresholds_and_unload_time() {
    let defaults = Config::default();
    assert_eq!(defaults.stage2.threads, 2);
    assert_eq!(defaults.priority, Priority::BelowNormal);
    assert_eq!(defaults.stage2.debounce, Duration::from_millis(30));
    assert_eq!(defaults.stage2.unload_after, Duration::from_secs(600));

    let config = Config::from_toml(
        r#"
        priority = "idle"
        k = 16
        [stage2]
        model = "model.gguf"
        threads = 4
        debounce_ms = 50
        skip_cost_gap = 1234
        unload_after_secs = 120
        "#,
    )
    .unwrap();
    assert_eq!(config.priority, Priority::Idle);
    assert_eq!(config.k, 16);
    assert_eq!(config.stage2.model, Some(PathBuf::from("model.gguf")));
    assert_eq!(config.stage2.threads, 4);
    assert_eq!(config.stage2.debounce, Duration::from_millis(50));
    assert_eq!(config.stage2.skip_cost_gap, 1234);
    assert_eq!(config.stage2.unload_after, Duration::from_secs(120));
}

#[test]
fn broken_config_falls_back_to_defaults_with_a_warning() {
    let dir = std::env::temp_dir().join(format!("aiokey-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "priority = [not toml").unwrap();
    let (config, warning) = Config::load(&path);
    assert_eq!(config.priority, Priority::BelowNormal);
    assert!(warning.unwrap().contains("config.toml"));
    std::fs::write(&path, "priority = \"fastest\"").unwrap();
    assert!(Config::load(&path).1.is_some());
}
