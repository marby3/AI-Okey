"""Issue #49：タグを打った版より、ワークスペースの版（Cargo.toml）が古くない。

#47 で版を上げ忘れ、v1.0.1 の Release が「tag v1.0.1 != v1.0.0」で止まった。
タグを取ってこないチェックアウト（CI の既定）では飛ばす。
"""

import re
import subprocess
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def parse(version):
    return tuple(int(x) for x in version.split("."))


def test_workspace_version_is_not_older_than_the_newest_tag():
    # dev コンテナでは、リポジトリの所有者が違うので safe.directory を付ける
    tags = subprocess.run(["git", "-c", f"safe.directory={ROOT}", "tag", "--list", "v*"],
                          cwd=ROOT, capture_output=True, text=True, check=True).stdout
    versions = [parse(m[1]) for m in re.finditer(r"^v(\d+\.\d+\.\d+)$", tags, re.M)]
    if not versions:
        pytest.skip("タグがない")
    with open(ROOT / "Cargo.toml", "rb") as f:
        version = tomllib.load(f)["workspace"]["package"]["version"]
    newest = max(versions)
    assert parse(version) >= newest, f"Cargo.toml {version} < tag v{'.'.join(map(str, newest))}"
