//! ランチャーは、起動できないプログラムがあっても止まらず、サーバーと候補ウィンドウの出力と終わり方をログに残す
//! （rc.1 で実機のサーバーが起動していなかったとき、原因を追えなかったため）。
//!
//! 本物のサーバーと候補ウィンドウの代わりに、同じ名前のシェルスクリプトを置いて確かめる。

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aiokey-launcher-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_script(path: &PathBuf, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn logs_the_output_and_exit_of_each_program_and_survives_a_missing_one() {
    let dir = temp_dir("missing-ui");
    let launcher = dir.join("aiokey-launcher");
    fs::copy(env!("CARGO_BIN_EXE_aiokey-launcher"), &launcher).unwrap();
    write_script(
        &dir.join("aiokey-server"),
        "echo listening\necho oops >&2\nexit 3",
    );
    // aiokey-ui は置かない（起動できない）

    let logs = dir.join("logs");
    let output = Command::new(&launcher)
        .env("AIOKEY_LOG_DIR", &logs)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let log = fs::read_to_string(logs.join("launcher.log")).unwrap();
    assert!(log.contains("[server]: listening"), "{log}");
    assert!(log.contains("[server]: oops"), "{log}");
    assert!(log.contains("[server] exited: exit status: 3"), "{log}");
    assert!(
        log.contains("[ui] failed to start") && log.contains("aiokey-ui"),
        "{log}"
    );
}

#[test]
fn a_second_launcher_exits_without_starting_anything() {
    // ログオン時の自動起動と、IME が最初のキー入力で起動するのが重なっても、サーバーは 1 つだけ動く
    let dir = temp_dir("single");
    let launcher = dir.join("aiokey-launcher");
    fs::copy(env!("CARGO_BIN_EXE_aiokey-launcher"), &launcher).unwrap();
    let starts = dir.join("starts");
    write_script(
        &dir.join("aiokey-server"),
        &format!("echo x >> {}\nsleep 3", starts.display()),
    );
    write_script(&dir.join("aiokey-ui"), "sleep 3");

    let logs = dir.join("logs");
    let mut first = Command::new(&launcher)
        .env("AIOKEY_LOG_DIR", &logs)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let begin = std::time::Instant::now();
    let second = Command::new(&launcher)
        .env("AIOKEY_LOG_DIR", &logs)
        .output()
        .unwrap();
    assert!(second.status.success(), "{second:?}");
    assert!(begin.elapsed() < std::time::Duration::from_secs(2));
    first.wait().unwrap();

    assert_eq!(fs::read_to_string(&starts).unwrap().lines().count(), 1);
    // 先に動いているランチャーのログを消さない
    let log = fs::read_to_string(logs.join("launcher.log")).unwrap();
    assert!(log.contains("[server] started"), "{log}");
}
