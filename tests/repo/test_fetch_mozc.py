"""Issue #3 基準 1：Mozc のデータを固定コミットから取得し、SHA-256 を検証する。"""

import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/fetch-mozc.sh"
SUMS = ROOT / "scripts/mozc.sha256"


def make_fake_data(tmp_path: Path) -> Path:
    """mozc.sha256 に載っている名前のファイルを作り、その中身に合わせたハッシュ一覧を返す。"""
    data = tmp_path / "mozc"
    data.mkdir()
    names = [line.split()[1] for line in SUMS.read_text().splitlines() if line.strip()]
    for name in names:
        (data / name).write_text(f"fake {name}\n")
    sums = subprocess.run(
        ["sha256sum", *names], cwd=data, check=True, capture_output=True, text=True
    ).stdout
    sums_file = tmp_path / "sums"
    sums_file.write_text(sums)
    return data, sums_file


def run_verify(data: Path, sums: Path):
    return subprocess.run(
        ["bash", str(SCRIPT), "--verify-only", "--dir", str(data), "--sums", str(sums)],
        capture_output=True,
        text=True,
    )


def test_fetch_script_pins_a_commit_and_verifies_sha256(tmp_path):
    script = SCRIPT.read_text()
    # コミットを 40 桁のハッシュで固定している
    assert any(len(tok) == 40 and all(c in "0123456789abcdef" for c in tok) for tok in script.split('"'))
    # 一覧に全ファイルがある
    names = {line.split()[1] for line in SUMS.read_text().splitlines() if line.strip()}
    assert {"id.def", "connection_single_column.txt", "README.txt"} <= names
    assert {f"dictionary0{i}.txt" for i in range(10)} <= names

    data, sums = make_fake_data(tmp_path)
    assert run_verify(data, sums).returncode == 0

    # 1 つでも中身が違えば失敗する
    (data / "id.def").write_text("tampered\n")
    result = run_verify(data, sums)
    assert result.returncode != 0
    assert "id.def" in result.stdout + result.stderr
