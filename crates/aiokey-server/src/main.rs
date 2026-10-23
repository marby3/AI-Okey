//! AI-O Key の変換サーバー。
//!
//!     aiokey-server [--config <config.toml>]
//!
//! 設定ファイルを省くと、Windows は `%APPDATA%\AI-O Key\config.toml`、unix は
//! `$XDG_CONFIG_HOME/aiokey/config.toml`（なければ `~/.config/aiokey/config.toml`）を読む。
//! ファイルがなければ既定値で動く。

use std::path::PathBuf;
use std::sync::Arc;

use aiokey_engine::dict::Dictionary;
use aiokey_ipc::proto::aiokey_service_server::AiokeyServiceServer;
use aiokey_server::config::{Config, Priority};
use aiokey_server::service::{model_loader, ConversionService};
use tonic::transport::Server;
use tonic_reflection::server::Builder as ReflectionBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path()?;
    let (config, warning) = Config::load(&path);
    if let Some(warning) = warning {
        eprintln!("aiokey-server: {warning}");
    }
    // Linux の nice はスレッドごとなので、tokio のスレッドを作る前に下げる（後から作るスレッドに引き継がれる）
    set_priority(config.priority);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(config))
}

fn config_path() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("--config"), Some(path)) => return Ok(PathBuf::from(path)),
        (None, _) => {}
        _ => return Err("usage: aiokey-server [--config <config.toml>]".into()),
    }
    #[cfg(windows)]
    let dir = std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("AI-O Key"));
    #[cfg(unix)]
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("aiokey"));
    match dir {
        Some(dir) => Ok(dir.join("config.toml")),
        None => {
            eprintln!("aiokey-server: no config directory (APPDATA / XDG_CONFIG_HOME / HOME); using ./config.toml");
            Ok(PathBuf::from("config.toml"))
        }
    }
}

#[cfg(unix)]
fn set_priority(priority: Priority) {
    let nice = match priority {
        Priority::Normal => 0,
        Priority::BelowNormal => 10,
        Priority::Idle => 19,
    };
    // SAFETY: 自分のプロセス（スレッド）の nice を変えるだけ
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) } != 0 {
        eprintln!(
            "aiokey-server: could not set the priority: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(windows)]
fn set_priority(priority: Priority) {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
        NORMAL_PRIORITY_CLASS,
    };
    let class = match priority {
        Priority::Normal => NORMAL_PRIORITY_CLASS,
        Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        Priority::Idle => IDLE_PRIORITY_CLASS,
    };
    // SAFETY: 自分のプロセスの優先度を変えるだけ
    if let Err(e) = unsafe { SetPriorityClass(GetCurrentProcess(), class) } {
        eprintln!("aiokey-server: could not set the priority: {e}");
    }
}

async fn serve(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // 辞書と学習済みの段2は、指定がなければ実行ファイルと同じフォルダのものを使う
    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("no parent directory of the executable")?
        .to_path_buf();
    let config = config.with_bundled_model(&exe_dir);
    let dictionary = match &config.dictionary {
        Some(path) => path.clone(),
        None => exe_dir.join("aiokey.dic"),
    };
    let dictionary = Arc::new(
        Dictionary::open(&dictionary).map_err(|e| format!("{}: {}", dictionary.display(), e.0))?,
    );
    let loader = model_loader(&config.stage2);
    let service = ConversionService::new(dictionary, config, loader);

    let reflection = ReflectionBuilder::configure()
        .register_encoded_file_descriptor_set(aiokey_ipc::proto::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let server = Server::builder()
        .add_service(AiokeyServiceServer::new(service))
        .add_service(reflection);

    #[cfg(windows)]
    {
        println!(r"aiokey-server listening on \\.\pipe\aiokey_server");
        server
            .serve_with_incoming(aiokey_server::TonicNamedPipeServer::new("aiokey_server"))
            .await?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        // 他のユーザーから接続されないよう、ユーザー専用の XDG_RUNTIME_DIR にだけ置く
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .map_err(|_| "XDG_RUNTIME_DIR is not set; refusing to create the socket elsewhere")?;
        let path = std::path::Path::new(&dir).join("aiokey_server.sock");
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        println!("aiokey-server listening on {}", path.display());
        server
            .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(listener))
            .await?;
    }

    Ok(())
}
