//! Issue #14 基準 5：Windows 版の段2を AVX2（x86-64-v3）でビルドし、実機で段2の時間を測れる。
//!
//! 辞書は Mozc のデータ（`scripts/fetch-mozc.sh`）から、段2は zenz の GGUF（`scripts/fetch-zenz.sh`）を使う。

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn windows_build_uses_avx2_and_bench_reports_stage2_time() {
    // x64 の Windows 向けは x86-64-v3（AVX2、FMA）でビルドする。llama-cpp-sys-2 はこの機能に合わせて GGML_AVX2 を有効にする
    let config = std::fs::read_to_string(root().join(".cargo/config.toml")).unwrap();
    let section = config
        .split("[target.x86_64-pc-windows-msvc]")
        .nth(1)
        .expect("[target.x86_64-pc-windows-msvc] in .cargo/config.toml");
    assert!(section.contains("target-cpu=x86-64-v3"), "{section}");

    let dir = std::env::temp_dir().join(format!("aiokey-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dict = dir.join("aiokey.dic");
    std::fs::write(
        &dict,
        aiokey_dictbuild::build_from_mozc(&root().join("data/mozc")).unwrap(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_aiokey-cli"))
        .args(["bench", "--iterations", "1", "--dict"])
        .arg(&dict)
        .arg("--stage2")
        .arg(root().join("data/models/zenz-v2.5-xsmall-q8_0.gguf"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for line in ["stage2 p50", "stage2 p95", "total p50", "total p95"] {
        assert!(stdout.contains(line), "{line}: {stdout}");
    }
}
