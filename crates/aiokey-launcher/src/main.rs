//! 変換サーバーと候補ウィンドウを起動する。ログオン時にユーザーの Run キーから動き（インストーラーが登録する）、
//! サーバーが動いていなければ IME も最初のキー入力で起動する。すでに動いていれば何もせずに終わる。
//!
//! 画面には何も出さないので、サーバーと候補ウィンドウの出力と終わり方は `launcher.log` に残す。
//! Windows は `%LOCALAPPDATA%\aiokey\logs`、unix は `$XDG_STATE_HOME/aiokey/logs`（なければ
//! `~/.local/state/aiokey/logs`）。`AIOKEY_LOG_DIR` で場所を変えられる。

// コンソールの窓を出さない
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

type Log = Arc<Mutex<File>>;

fn main() -> anyhow::Result<()> {
    // サーバーと UI はランチャーと同じフォルダに置く
    let exe_dir = env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory of the launcher"))?
        .to_path_buf();
    let exe = |name: &str| exe_dir.join(format!("{}{}", name, env::consts::EXE_SUFFIX));

    let dir = log_dir();
    fs::create_dir_all(&dir)?;
    // ログオン時の自動起動と IME からの起動が重なることがあるので、1 つだけ動かす。
    // ロックはランチャーが終わるまで持つ（プロセスが終われば OS が外す）
    // （ロックされたファイルは Windows では切り詰められないので、File::create は使わない）
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("launcher.lock"))?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let log: Log = Arc::new(Mutex::new(File::create(dir.join("launcher.log"))?));
    write_line(
        &log,
        &format!("aiokey-launcher {}", env!("CARGO_PKG_VERSION")),
    );

    let children: Vec<_> = [("aiokey-server", "[server]"), ("aiokey-ui", "[ui]")]
        .into_iter()
        .filter_map(|(name, prefix)| start_process(&exe(name), prefix, &log).map(|c| (c, prefix)))
        .map(|((mut child, forwarders), prefix)| {
            let log = log.clone();
            thread::spawn(move || {
                let status = child.wait();
                // 出力を書き終えてから、終わったことを書く
                for forwarder in forwarders {
                    let _ = forwarder.join();
                }
                let status = match status {
                    Ok(status) => status.to_string(),
                    Err(e) => format!("unknown ({e})"),
                };
                write_line(&log, &format!("{prefix} exited: {status}"));
            })
        })
        .collect();
    for child in children {
        let _ = child.join();
    }

    Ok(())
}

fn log_dir() -> PathBuf {
    if let Some(dir) = env::var_os("AIOKEY_LOG_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    let base = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(unix)]
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")));
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("aiokey")
        .join("logs")
}

fn write_line(log: &Log, line: &str) {
    if let Ok(mut file) = log.lock() {
        let _ = writeln!(file, "{line}");
    }
}

/// 起動したプログラムと、その出力をログへ写すスレッドを返す。
/// 起動できなかったときは、ログに残して None を返す（ほかのプログラムは起動する）
fn start_process(exe: &Path, prefix: &str, log: &Log) -> Option<(Child, Vec<JoinHandle<()>>)> {
    let mut command = Command::new(exe);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // サーバーと UI はコンソールのプログラムなので、窓を出さないように起動する
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            write_line(
                log,
                &format!("{prefix} failed to start {}: {e}", exe.display()),
            );
            return None;
        }
    };
    write_line(
        log,
        &format!("{prefix} started {} (pid {})", exe.display(), child.id()),
    );

    let mut forwarders = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        forwarders.push(forward(stdout, prefix, log));
    }
    if let Some(stderr) = child.stderr.take() {
        forwarders.push(forward(stderr, prefix, log));
    }
    Some((child, forwarders))
}

fn forward(stream: impl Read + Send + 'static, prefix: &str, log: &Log) -> JoinHandle<()> {
    let prefix = prefix.to_string();
    let log = log.clone();
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            write_line(&log, &format!("{prefix}: {line}"));
        }
    })
}
