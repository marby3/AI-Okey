use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::{env, thread};

fn main() -> anyhow::Result<()> {
    // サーバーと UI はランチャーと同じフォルダに置く
    let exe_dir = env::current_exe()?.parent().unwrap().to_path_buf();
    let exe = |name: &str| exe_dir.join(format!("{}{}", name, env::consts::EXE_SUFFIX));

    let server_process = start_process(&exe("aiokey-server"), "[server]");
    let ui_process = start_process(&exe("aiokey-ui"), "[ui]");

    if let (Some(mut server), Some(mut ui)) = (server_process, ui_process) {
        let server_handle = thread::spawn(move || server.wait());
        let ui_handle = thread::spawn(move || ui.wait());

        let _ = server_handle.join();
        let _ = ui_handle.join();
    }

    Ok(())
}

fn start_process(exe: &Path, prefix: &str) -> Option<Child> {
    let mut child = Command::new(exe)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect(&format!("Failed to start {}", exe.display()));

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stdout_reader = BufReader::new(stdout);
    let prefix_stdout = prefix.to_string();
    thread::spawn(move || {
        for line in stdout_reader.lines() {
            if let Ok(line) = line {
                println!("{}: {}", prefix_stdout, line);
            }
        }
    });

    let stderr = child.stderr.take().expect("Failed to capture stderr");
    let stderr_reader = BufReader::new(stderr);
    let prefix_stderr = prefix.to_string();
    thread::spawn(move || {
        for line in stderr_reader.lines() {
            if let Ok(line) = line {
                eprintln!("{}: {}", prefix_stderr, line);
            }
        }
    });

    Some(child)
}
