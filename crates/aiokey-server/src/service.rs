//! 変換サービス（OS に依存しない）。
//!
//! 入力（スケルトン）は 1 つのセッションに持つ。要求ごとに段1（辞書と n-gram）でその場で候補を出し、
//! 段2（AI 採点器）は裏で動かす。PC の負荷を抑えるため、段2は次のときだけ動かす。
//!
//! - キー入力が `debounce` のあいだ止まったとき、と変換キー（`Convert`）のとき
//! - 段1の 1 位と 2 位のコスト差が `skip_cost_gap` より小さい（段1が迷っている）とき
//!
//! 段2の結果は、入力の番号（revision）が採点を始めたときと同じときだけ使い、古い入力の結果は捨てる。
//! 段2のモデルは `unload_after` のあいだ使わなければ解放し、次に要るときに読み込み直す。

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use aiokey_engine::convert::{convert, Candidate, Options};
use aiokey_engine::dict::Dictionary;
use aiokey_ipc::proto::{
    aiokey_service_server::AiokeyService, AppendTextRequest, AppendTextResponse, ClearTextRequest,
    ClearTextResponse, ComposingText, ConvertRequest, ConvertResponse, GetComposingTextRequest,
    GetComposingTextResponse, MoveCursorRequest, MoveCursorResponse, RemoveTextRequest,
    RemoveTextResponse, SetContextRequest, SetContextResponse, ShrinkTextRequest,
    ShrinkTextResponse, Suggestion, UpdateConfigRequest, UpdateConfigResponse,
};
use tokio::sync::watch;
use tonic::{Request, Response, Status};

use crate::config::{Config, Stage2Config};

/// GetComposingText と Convert で、段2の結果を待つ時間の上限。
const MAX_WAIT: Duration = Duration::from_secs(10);

/// 段2の採点器。候補（表記, 読み）ごとの点数（大きいほどよい）を返す。
pub trait Reranker: Send {
    fn score(
        &mut self,
        context: &str,
        input: &str,
        candidates: &[(String, String)],
    ) -> Result<Vec<f32>, String>;
}

/// 段2の採点器を読み込む関数。モデルを解放したあと、次に要るときにも呼ぶ。
pub type Loader = Arc<dyn Fn() -> Result<Box<dyn Reranker>, String> + Send + Sync>;

/// aiokey-scorer（GGUF）を読み込む [`Loader`]。設定に段2のモデルがなければ None。
pub fn model_loader(config: &Stage2Config) -> Option<Loader> {
    let path = config.model.clone()?;
    let scorer_config = aiokey_scorer::Config {
        threads: config.threads,
        format: match config.format.as_str() {
            "zenz" => aiokey_scorer::PromptFormat::ZenzV25,
            _ => aiokey_scorer::PromptFormat::Aiokey,
        },
        max_candidates: config.max_candidates,
        temperature: config.temperature,
    };
    Some(Arc::new(move || {
        let scorer = aiokey_scorer::Scorer::open(&path, &scorer_config)?;
        Ok(Box::new(ModelReranker(scorer)) as Box<dyn Reranker>)
    }))
}

struct ModelReranker(aiokey_scorer::Scorer);

impl Reranker for ModelReranker {
    fn score(
        &mut self,
        context: &str,
        input: &str,
        candidates: &[(String, String)],
    ) -> Result<Vec<f32>, String> {
        let req = aiokey_scorer::Request {
            context,
            input,
            candidates: candidates
                .iter()
                .map(|(s, r)| (s.as_str(), r.as_str()))
                .collect(),
        };
        Ok(self.0.score(&req)?.probs)
    }
}

#[derive(Default)]
struct Session {
    input: Vec<char>,
    /// カーソルの位置（文字数）。候補はカーソルより前の入力から作る
    cursor: usize,
    context: String,
    /// 入力か文脈が変わるたびに増える
    revision: u64,
    /// 段1の候補（カーソルより前の入力の）
    candidates: Vec<Candidate>,
    /// 段2の並び（candidates の添字）。今の revision のものだけを持つ
    order: Option<Vec<usize>>,
    /// 今の revision について、段2が済んだ（または呼ばないと決めた）か
    settled: bool,
    /// 採点中の revision（同じ入力を 2 回採点しないため）
    scoring: Option<u64>,
}

struct Stage2 {
    loader: Loader,
    reranker: Mutex<Option<Box<dyn Reranker>>>,
    last_used: Mutex<Instant>,
}

struct Inner {
    dictionary: Arc<Dictionary>,
    config: Config,
    session: Mutex<Session>,
    /// 入力が変わったことを、段2の待ち合わせのタスクに知らせる
    changed: watch::Sender<u64>,
    /// 段2が済んだ revision を、GetComposingText で待っている要求に知らせる
    settled: watch::Sender<u64>,
    stage2: Option<Stage2>,
}

/// 変換サービス。作ると、段2の待ち合わせとモデルの解放のタスクを tokio の上で動かす（サービスを捨てると止まる）。
pub struct ConversionService {
    inner: Arc<Inner>,
}

impl ConversionService {
    /// `loader` が None なら段2を使わない。tokio のランタイムの中で呼ぶ。
    pub fn new(dictionary: Arc<Dictionary>, config: Config, loader: Option<Loader>) -> Self {
        let (changed, changed_rx) = watch::channel(0);
        let (settled, _) = watch::channel(0);
        let inner = Arc::new(Inner {
            dictionary,
            config,
            session: Mutex::new(Session {
                settled: true,
                ..Session::default()
            }),
            changed,
            settled,
            stage2: loader.map(|loader| Stage2 {
                loader,
                reranker: Mutex::new(None),
                last_used: Mutex::new(Instant::now()),
            }),
        });
        if inner.stage2.is_some() {
            tokio::spawn(debounce(Arc::downgrade(&inner), changed_rx));
            tokio::spawn(unload_when_idle(Arc::downgrade(&inner)));
        }
        Self { inner }
    }

    /// 段2のモデルを読み込んであるか。
    pub fn stage2_loaded(&self) -> bool {
        self.inner
            .stage2
            .as_ref()
            .is_some_and(|s| s.reranker.lock().unwrap().is_some())
    }

    /// 入力を変えて段1をやり直し、応答を返す。
    fn edit(&self, f: impl FnOnce(&mut Session)) -> ComposingText {
        let mut session = self.inner.session.lock().unwrap();
        let before = (
            session.input.clone(),
            session.cursor,
            session.context.clone(),
        );
        f(&mut session);
        session.cursor = session.cursor.min(session.input.len());
        // 何も変わらなければ（先頭での RemoveText など）、計算し直さない
        if (&session.input, session.cursor, &session.context) == (&before.0, before.1, &before.2) {
            return composing_text(&session);
        }
        let text: String = session.input[..session.cursor].iter().collect();
        session.candidates = if text.is_empty() {
            Vec::new()
        } else {
            convert(
                &self.inner.dictionary,
                &text,
                &Options {
                    k: self.inner.config.k,
                },
            )
        };
        session.revision += 1;
        session.order = None;
        session.settled = self.inner.stage2.is_none() || session.candidates.is_empty();
        let response = composing_text(&session);
        if !session.settled {
            let _ = self.inner.changed.send(session.revision);
        }
        response
    }

    fn current(&self) -> ComposingText {
        composing_text(&self.inner.session.lock().unwrap())
    }

    /// 今の入力について段2が済む（または呼ばないと決まる）まで、`wait` を上限に待つ。
    async fn wait_settled(&self, wait: Duration) {
        let mut settled = self.inner.settled.subscribe();
        let _ = tokio::time::timeout(wait.min(MAX_WAIT), async {
            loop {
                if self.inner.session.lock().unwrap().settled {
                    return;
                }
                if settled.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
    }
}

fn composing_text(session: &Session) -> ComposingText {
    let identity: Vec<usize> = (0..session.candidates.len()).collect();
    let order = session.order.as_ref().unwrap_or(&identity);
    let count = session.cursor as i32;
    ComposingText {
        hiragana: order
            .first()
            .map(|&i| session.candidates[i].reading.clone())
            .unwrap_or_default(),
        suggestions: order
            .iter()
            .map(|&i| Suggestion {
                text: session.candidates[i].surface.clone(),
                subtext: String::new(),
                corresponding_count: count,
            })
            .collect(),
        revision: session.revision,
        reranked: session.order.is_some(),
        margin: 0.0,
        uncertain: false,
        settled: false,
    }
}

/// Kana モードの TSF は英字を全角で送ってくるので、全角の ASCII（！〜～）を半角に直す。
fn to_halfwidth(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().map(|c| match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    })
}

/// キー入力が `debounce` のあいだ止まったら、段2を動かす。
async fn debounce(inner: Weak<Inner>, mut changed: watch::Receiver<u64>) {
    let wait = match inner.upgrade() {
        Some(inner) => inner.config.stage2.debounce,
        None => return,
    };
    // サービスを捨てると送り手がなくなり、changed() がエラーになって止まる
    while changed.changed().await.is_ok() {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(wait) => break,
                r = changed.changed() => if r.is_err() { return },
            }
        }
        let revision = *changed.borrow_and_update();
        let Some(inner) = inner.upgrade() else { return };
        run_stage2(&inner, revision).await;
    }
}

/// 段2のモデルを `unload_after` のあいだ使わなければ解放する。
async fn unload_when_idle(inner: Weak<Inner>) {
    loop {
        let wait = {
            let Some(inner) = inner.upgrade() else { return };
            let stage2 = inner.stage2.as_ref().expect("stage2 is enabled");
            let after = inner.config.stage2.unload_after;
            // 使っていない時間は、採点器を借りてから測る（測ってから借りると、その間に済んだ採点を見落とす）。
            // 採点中で借りられなければ、次の回に回す
            let wait = match stage2.reranker.try_lock() {
                Ok(mut reranker) => {
                    let idle = stage2.last_used.lock().unwrap().elapsed();
                    if idle >= after {
                        *reranker = None;
                        after
                    } else {
                        after - idle
                    }
                }
                Err(_) => after,
            };
            wait
        };
        tokio::time::sleep(wait.max(Duration::from_millis(10))).await;
    }
}

/// `revision` の入力を段2で並べ替える。段1が迷っていなければ呼ばない。途中で入力が変わったら結果を捨てる。
async fn run_stage2(inner: &Arc<Inner>, revision: u64) {
    if inner.stage2.is_none() {
        return;
    }
    let job = {
        let mut session = inner.session.lock().unwrap();
        // 済んでいるか、ほかの要求が採点中なら何もしない（Convert はその結果を待つ）
        if session.revision != revision || session.settled || session.scoring == Some(revision) {
            return;
        }
        let c = &session.candidates;
        let confident =
            c.len() < 2 || c[1].cost.saturating_sub(c[0].cost) >= inner.config.stage2.skip_cost_gap;
        if confident {
            session.settled = true;
            let _ = inner.settled.send(revision);
            return;
        }
        let n = c.len().min(inner.config.stage2.max_candidates.max(1));
        let pairs: Vec<(String, String)> = c[..n]
            .iter()
            .map(|c| (c.surface.clone(), c.reading.clone()))
            .collect();
        let input: String = session.input[..session.cursor].iter().collect();
        session.scoring = Some(revision);
        (session.context.clone(), input, pairs)
    };

    let task_inner = inner.clone();
    let scores = tokio::task::spawn_blocking(move || {
        let stage2 = task_inner.stage2.as_ref().expect("stage2 is enabled");
        let (context, input, pairs) = job;
        let mut reranker = stage2.reranker.lock().unwrap();
        if reranker.is_none() {
            *reranker = Some((stage2.loader)()?);
        }
        let scores = reranker.as_mut().unwrap().score(&context, &input, &pairs);
        *stage2.last_used.lock().unwrap() = Instant::now();
        scores
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));

    let mut session = inner.session.lock().unwrap();
    if session.scoring == Some(revision) {
        session.scoring = None;
    }
    if session.revision != revision {
        return; // 古い入力の結果は捨てる
    }
    match scores {
        Ok(scores) => {
            let mut order: Vec<usize> = (0..scores.len()).collect();
            order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
            // 段2で採点しなかった候補は、段1の並びのまま後ろに置く
            order.extend(scores.len()..session.candidates.len());
            session.order = Some(order);
        }
        Err(e) => eprintln!("aiokey-server: stage 2 failed: {e}"),
    }
    session.settled = true;
    let _ = inner.settled.send(revision);
}

#[tonic::async_trait]
impl AiokeyService for ConversionService {
    async fn append_text(
        &self,
        request: Request<AppendTextRequest>,
    ) -> Result<Response<AppendTextResponse>, Status> {
        let text = request.into_inner().text_to_append;
        if text.is_empty() {
            return Err(Status::invalid_argument("text_to_append is empty"));
        }
        let composing_text = self.edit(|s| {
            let added: Vec<char> = to_halfwidth(&text).collect();
            let at = s.cursor.min(s.input.len());
            s.cursor = at + added.len();
            s.input.splice(at..at, added);
        });
        Ok(Response::new(AppendTextResponse {
            composing_text: Some(composing_text),
        }))
    }

    async fn remove_text(
        &self,
        _: Request<RemoveTextRequest>,
    ) -> Result<Response<RemoveTextResponse>, Status> {
        let composing_text = self.edit(|s| {
            if s.cursor > 0 {
                s.cursor -= 1;
                s.input.remove(s.cursor);
            }
        });
        Ok(Response::new(RemoveTextResponse {
            composing_text: Some(composing_text),
        }))
    }

    /// 先頭の `offset` 文字（確定した部分）を入力から落とす。
    async fn shrink_text(
        &self,
        request: Request<ShrinkTextRequest>,
    ) -> Result<Response<ShrinkTextResponse>, Status> {
        let offset = usize::try_from(request.into_inner().offset)
            .map_err(|_| Status::invalid_argument("offset is negative"))?;
        let composing_text = self.edit(|s| {
            let n = offset.min(s.input.len());
            s.input.drain(..n);
            s.cursor = s.cursor.saturating_sub(n);
        });
        Ok(Response::new(ShrinkTextResponse {
            composing_text: Some(composing_text),
        }))
    }

    /// カーソルを `offset` 文字だけ動かす（負なら前へ）。
    async fn move_cursor(
        &self,
        request: Request<MoveCursorRequest>,
    ) -> Result<Response<MoveCursorResponse>, Status> {
        let offset = request.into_inner().offset as isize;
        let composing_text = self.edit(|s| {
            s.cursor = s.cursor.saturating_add_signed(offset).min(s.input.len());
        });
        Ok(Response::new(MoveCursorResponse {
            composing_text: Some(composing_text),
        }))
    }

    async fn clear_text(
        &self,
        _: Request<ClearTextRequest>,
    ) -> Result<Response<ClearTextResponse>, Status> {
        self.edit(|s| {
            s.input.clear();
            s.cursor = 0;
        });
        Ok(Response::new(ClearTextResponse {}))
    }

    async fn set_context(
        &self,
        request: Request<SetContextRequest>,
    ) -> Result<Response<SetContextResponse>, Status> {
        let context = request.into_inner().context;
        self.edit(|s| s.context = context);
        Ok(Response::new(SetContextResponse {}))
    }

    async fn update_config(
        &self,
        _: Request<UpdateConfigRequest>,
    ) -> Result<Response<UpdateConfigResponse>, Status> {
        Ok(Response::new(UpdateConfigResponse {}))
    }

    async fn convert(
        &self,
        _: Request<ConvertRequest>,
    ) -> Result<Response<ConvertResponse>, Status> {
        let revision = self.inner.session.lock().unwrap().revision;
        run_stage2(&self.inner, revision).await;
        // 裏で同じ入力を採点中なら、その結果を待つ
        self.wait_settled(MAX_WAIT).await;
        Ok(Response::new(ConvertResponse {
            composing_text: Some(self.current()),
        }))
    }

    async fn get_composing_text(
        &self,
        request: Request<GetComposingTextRequest>,
    ) -> Result<Response<GetComposingTextResponse>, Status> {
        let wait = Duration::from_millis(request.into_inner().wait_ms as u64);
        self.wait_settled(wait).await;
        Ok(Response::new(GetComposingTextResponse {
            composing_text: Some(self.current()),
        }))
    }
}
