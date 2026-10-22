"""Issue #37 基準 1：Windows の CI と Release は、protoc を版を固定して直接取り、SHA-256 を確かめる。

`arduino/setup-protoc` は版を指定しないと 23.x を探し、見つからなくなって失敗した（v0.12.0 の Release）。
"""

import re
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
    assert "https://github.com/protocolbuffers/protobuf/releases/download/" in script
    assert "sha256sum -c" in script
