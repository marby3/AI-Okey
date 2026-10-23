"""Issue #13 基準 5：学習済みの段2（GGUF）と校正後の温度を、Release に添付する。

モデルはリポジトリに入れず、モデル専用の Release に置いて SHA-256 を固定し、Release のワークフローで取ってくる。
"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RELEASE = ROOT / ".github/workflows/release.yml"
SCRIPT = ROOT / "scripts/fetch-stage2-model.sh"


def test_release_attaches_the_trained_stage2_model_and_temperature():
    script = SCRIPT.read_text()
    assert re.search(r'^MODEL_TAG="stage2-model-v\d+"$', script, re.M)
    assert len(re.findall(r'^(GGUF|INFO)_SHA256="[0-9a-f]{64}"$', script, re.M)) == 2
    assert "sha256sum -c" in script
    release = RELEASE.read_text()
    assert "scripts/fetch-stage2-model.sh" in release
    # 配布物の zip に入れ、Release にも添付する
    package = release[release.index("name: Package") :]
    assert "aiokey-stage2-q8_0.gguf" in package and "aiokey-stage2.json" in package
    create = release[release.index("gh release create") :].split("\n\n")[0]
    assert "aiokey-stage2-q8_0.gguf" in create and "aiokey-stage2.json" in create


def test_tampered_stage2_model_fails_verification(tmp_path):
    for name in ["aiokey-stage2-q8_0.gguf", "aiokey-stage2.json"]:
        (tmp_path / name).write_bytes(b"tampered")
    result = subprocess.run(["bash", str(SCRIPT), "--verify-only", str(tmp_path)], capture_output=True, text=True)
    assert result.returncode != 0
    assert "aiokey-stage2" in result.stdout + result.stderr
