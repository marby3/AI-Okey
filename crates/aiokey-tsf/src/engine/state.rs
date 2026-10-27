use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use windows::{core::GUID, Win32::UI::TextServices::ITfContext};

use super::{input_mode::InputMode, ipc_service::IPCService};
use crate::globals::DllModule;

#[derive(Debug)]
pub struct IMEState {
    pub ipc_service: Option<IPCService>,
    /// 最後に変換サーバーへつなごうとして失敗した時刻
    pub last_connect_failure: Option<Instant>,
    /// 最後にランチャーを起動した時刻
    pub last_launch: Option<Instant>,
    pub input_mode: InputMode,
    pub cookies: HashMap<GUID, u32>,
    pub context: Option<ITfContext>,
}

pub static IME_STATE: LazyLock<Mutex<IMEState>> = LazyLock::new(|| {
    tracing::debug!("Creating IMEState");
    Mutex::new(IMEState {
        ipc_service: None,
        last_connect_failure: None,
        last_launch: None,
        input_mode: InputMode::default(),
        cookies: HashMap::new(),
        context: None,
    })
});
unsafe impl Sync for IMEState {}
unsafe impl Send for IMEState {}

/// つながらなかったあと、つなぎ直すまでの間隔（キー入力のたびに試して遅くならないように）
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
/// ランチャーを起動し直すまでの間隔（サーバーが立ち上がるのを待つ）
const RELAUNCH_INTERVAL: Duration = Duration::from_secs(10);

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
                // 自動起動に失敗していても使えるよう、ランチャーを起動する（動いていれば、ランチャーはすぐ終わる）
                if state
                    .last_launch
                    .is_none_or(|t| t.elapsed() >= RELAUNCH_INTERVAL)
                {
                    state.last_launch = Some(Instant::now());
                    drop(state);
                    if let Err(e) = start_launcher() {
                        tracing::error!("Failed to start the launcher: {e:?}");
                    }
                }
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

/// この DLL と同じフォルダの aiokey-launcher.exe を起動する（x86 の DLL も同じフォルダに置く）
fn start_launcher() -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt as _;
    use windows::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB;

    let dll = std::path::PathBuf::from(DllModule::get_path()?);
    let launcher = dll
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory of {}", dll.display()))?
        .join("aiokey-launcher.exe");
    let mut command = std::process::Command::new(&launcher);
    // アプリがジョブで子プロセスをまとめて終わらせることがあるので、ジョブから切り離す。
    // 切り離しを許さないジョブでは、そのまま起動する
    let started = command
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB.0)
        .spawn()
        .or_else(|_| command.creation_flags(0).spawn());
    started.map_err(|e| anyhow::anyhow!("{}: {e}", launcher.display()))?;
    tracing::debug!("Started {}", launcher.display());
    Ok(())
}
