//! Issue #10 基準 6：unix ではソケット（UDS）で同じサービスが動く。
//!
//! サーバーのバイナリを起動し、XDG_RUNTIME_DIR に作られるソケットへ gRPC でつなぐ。
//! 辞書は Mozc のデータ（`scripts/fetch-mozc.sh` で取得）から作る。
#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use aiokey_ipc::proto::{aiokey_service_client::AiokeyServiceClient, AppendTextRequest};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tonic::transport::Endpoint;

fn mozc_dir() -> PathBuf {
    std::env::var_os("AIOKEY_MOZC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/mozc"))
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// /proc/<pid>/stat の nice の値（19 番目の欄）。
fn nice_of(pid: u32) -> i32 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    // 2 番目の欄（プロセス名）は括弧つきで空白を含みうるので、閉じ括弧のあとから数える
    let rest = &stat[stat.rfind(')').unwrap() + 2..];
    rest.split(' ').nth(16).unwrap().parse().unwrap()
}

#[tokio::test]
async fn unix_socket_serves_the_same_service() {
    let dir = std::env::temp_dir().join(format!("aiokey-uds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dict = dir.join("aiokey.dic");
    std::fs::write(
        &dict,
        aiokey_dictbuild::build_from_mozc(&mozc_dir())
            .expect("scripts/fetch-mozc.sh を実行したか確かめる"),
    )
    .unwrap();
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        format!("dictionary = {:?}\n", dict.display().to_string()),
    )
    .unwrap();

    let socket = dir.join("aiokey_server.sock");
    let _ = std::fs::remove_file(&socket);
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_aiokey-server"))
            .arg("--config")
            .arg(&config)
            .env("XDG_RUNTIME_DIR", &dir)
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while !socket.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "socket was not created"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 既定の優先度（BELOW_NORMAL）は、unix では nice 10 にする
    assert_eq!(nice_of(server.0.id()), 10);

    let path = socket.clone();
    let channel = Endpoint::try_from("http://[::]:50051")
        .unwrap()
        .connect_with_connector(tower::service_fn(move |_| {
            let path = path.clone();
            async move { Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(path).await?)) }
        }))
        .await
        .unwrap();
    let mut client = AiokeyServiceClient::new(channel);
    let mut last = None;
    for c in "htsthqtun".chars() {
        last = client
            .append_text(AppendTextRequest {
                text_to_append: c.to_string(),
            })
            .await
            .unwrap()
            .into_inner()
            .composing_text;
    }
    let texts: Vec<String> = last
        .unwrap()
        .suggestions
        .into_iter()
        .map(|s| s.text)
        .collect();
    assert!(texts.iter().any(|t| t == "果たして本当に"), "{texts:?}");
}
