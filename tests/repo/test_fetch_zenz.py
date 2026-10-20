"""Issue #8 基準 1：zenz-v2.5-xsmall を固定リビジョンで取得し、SHA-256 を検証する。"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/fetch-zenz.sh"
SUMS = ROOT / "scripts/zenz.sha256"


def test_fetch_script_pins_a_revision_and_verifies_sha256(tmp_path):
    script = SCRIPT.read_text()
    assert re.search(r'^ZENZ_REVISION="[0-9a-f]{40}"$', script, re.M)
    assert re.search(r'^BASE_URL="https://huggingface\.co/\$\{ZENZ_REPO\}/resolve/\$\{ZENZ_REVISION\}"$', script, re.M)
    assert "--outtype" in (ROOT / "scripts/convert_zenz.py").read_text() and "q8_0" in (ROOT / "scripts/convert_zenz.py").read_text()

    names = [line.split()[1] for line in SUMS.read_text().splitlines() if line.strip()]
    assert "model.safetensors" in names
    data = tmp_path / "zenz"
    data.mkdir()
    for name in names:
        (data / name).write_text(f"fake {name}\n")
    sums = tmp_path / "sums"
    sums.write_text(subprocess.run(["sha256sum", *names], cwd=data, check=True, capture_output=True, text=True).stdout)
    run = lambda: subprocess.run(
        ["bash", str(SCRIPT), "--verify-only", "--dir", str(data), "--sums", str(sums)], capture_output=True, text=True
    )
    assert run().returncode == 0
    (data / "model.safetensors").write_text("tampered\n")
    result = run()
    assert result.returncode != 0 and "model.safetensors" in result.stdout + result.stderr
