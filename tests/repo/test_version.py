"""Issue #49：タグを打った版より、ワークスペースの版（Cargo.toml）が古くない。

#47 で版を上げ忘れ、v1.0.1 の Release が「tag v1.0.1 != v1.0.0」で止まった。
タグを取ってこないチェックアウト（CI の既定）では飛ばす。
"""

import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def parse(version):
    return tuple(int(x) for x in version.split("."))


def test_workspace_version_is_not_older_than_the_newest_tag():
    tags = subprocess.run(["git", "tag", "--list", "v*"], cwd=ROOT, capture_output=True, text=True).stdout
    versions = [parse(m[1]) for m in re.finditer(r"^v(\d+\.\d+\.\d+)$", tags, re.M)]
    if not versions:
        pytest.skip("タグがない")
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    workspace = parse(re.search(r'^version = "(\d+\.\d+\.\d+)"', cargo, re.M)[1])
    assert workspace >= max(versions), f"Cargo.toml {workspace} < tag {max(versions)}"
