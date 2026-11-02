use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use windows::{core::GUID, Win32::UI::TextServices::ITfContext};

use super::{input_mode::InputMode, ipc_service::IPCService};

#[derive(Debug)]
pub struct IMEState {
    pub ipc_service: Option<IPCService>,
    /// 最後に変換サーバーへつなごうとして失敗した時刻
    pub last_connect_failure: Option<Instant>,
    pub input_mode: InputMode,
    pub cookies: HashMap<GUID, u32>,
    pub context: Option<ITfContext>,
}

pub static IME_STATE: LazyLock<Mutex<IMEState>> = LazyLock::new(|| {
    tracing::debug!("Creating IMEState");
    Mutex::new(IMEState {
        ipc_service: None,
        last_connect_failure: None,
        input_mode: InputMode::default(),
        cookies: HashMap::new(),
        context: None,
    })
});
unsafe impl Sync for IMEState {}
unsafe impl Send for IMEState {}

/// つながらなかったあと、つなぎ直すまでの間隔（キー入力のたびに試して遅くならないように）
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);

impl IMEState {
    /// 変換サーバーと候補ウィンドウへの接続を返す。まだつながっていなければつなぐ
    /// （サーバーが IME の有効化より後に起動したときも使えるように）。つながらなければ None
    pub fn connect() -> anyhow::Result<Option<IPCService>> {
        {
            let state = Self::get()?;
            if let Some(ipc_service) = &state.ipc_service {
                return Ok(Some(ipc_service.clone()));
            }
            if state
                .last_connect_failure
                .is_some_and(|t| t.elapsed() < RECONNECT_INTERVAL)
            {
                return Ok(None);
            }
        }
        // つなぐ間は状態のロックを持たない
        let connected = IPCService::new();
        let mut state = Self::get()?;
        match connected {
            Ok(ipc_service) => {
                tracing::debug!("Connected to the server");
                state.last_connect_failure = None;
                state.ipc_service = Some(ipc_service.clone());
                Ok(Some(ipc_service))
            }
            Err(e) => {
                tracing::error!("Failed to connect to the server: {e:?}");
                state.last_connect_failure = Some(Instant::now());
                Ok(None)
            }
        }
    }

    pub fn get() -> anyhow::Result<MutexGuard<'static, IMEState>> {
        match IME_STATE.try_lock() {
            Ok(guard) => Ok(guard),
            Err(e) => anyhow::bail!("Failed to lock state: {:?}", e),
        }
    }
}
