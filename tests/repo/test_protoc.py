"""Issue #37 基準 1：Windows の CI と Release は、protoc を版を固定して直接取り、SHA-256 を確かめる。

`arduino/setup-protoc` は版を指定しないと 23.x を探し、見つからなくなって失敗した（v0.12.0 の Release）。
"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = [ROOT / ".github/workflows/ci.yml", ROOT / ".github/workflows/release.yml"]
SCRIPT = ROOT / "scripts/fetch-protoc.sh"


def test_windows_jobs_fetch_a_pinned_protoc_and_verify_it():
    for workflow in WORKFLOWS:
        text = workflow.read_text()
        assert "setup-protoc" not in text, workflow.name
        assert "scripts/fetch-protoc.sh" in text, workflow.name
    script = SCRIPT.read_text()
    assert re.search(r'^PROTOC_VERSION="\d+\.\d+"$', script, re.M)
    assert re.search(r'^PROTOC_SHA256="[0-9a-f]{64}"$', script, re.M)
    # 取ってくるのは固定した版
    assert "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/" in script
    assert "sha256sum -c" in script
    # Rust のビルドより前に入れる
    for workflow in WORKFLOWS:
        text = workflow.read_text()
        assert text.index("scripts/fetch-protoc.sh") < text.index("cargo build"), workflow.name


def test_tampered_protoc_archive_fails_verification(tmp_path):
    archive = tmp_path / "protoc.zip"
    archive.write_bytes(b"tampered")
    result = subprocess.run(["bash", str(SCRIPT), "--verify-only", str(archive)], capture_output=True, text=True)
    assert result.returncode != 0
    assert "protoc.zip" in result.stdout + result.stderr
