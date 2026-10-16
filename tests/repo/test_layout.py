"""リポジトリ構成の受け入れテスト（Issue #1）。"""

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# azooKey-Windows の TSF / プロファイル / 表示属性 GUID の先頭部分
UPSTREAM_GUID_PREFIXES = ["ffdefe79", "ffdefe7a", "ffdefe7b"]

RENAMED_CRATES = {
    "crates/aiokey-tsf": "aiokey-tsf",
    "crates/aiokey-server": "aiokey-server",
    "crates/aiokey-ui": "aiokey-ui",
    "crates/aiokey-launcher": "aiokey-launcher",
    "crates/aiokey-ipc": "aiokey-ipc",
    "crates/aiokey-macros": "aiokey-macros",
}

CODE_DIRS = ["crates", "installer", "scripts", ".github", "docker"]
CODE_SUFFIXES = {".rs", ".toml", ".proto", ".rc", ".h", ".iss", ".xml", ".yml", ".yaml", ".bat", ".sh", ".ps1"}


def load_toml(path: Path) -> dict:
    with path.open("rb") as f:
        return tomllib.load(f)


def test_swift_engine_and_tauri_app_are_removed():
    for path in ["server-swift", "frontend", ".gitmodules"]:
        assert not (ROOT / path).exists(), path
    members = load_toml(ROOT / "Cargo.toml")["workspace"]["members"]
    assert not [m for m in members if "server-swift" in m or "frontend" in m]


def test_crates_and_identifiers_are_renamed_to_aiokey():
    members = load_toml(ROOT / "Cargo.toml")["workspace"]["members"]
    for crate_dir, package in RENAMED_CRATES.items():
        assert crate_dir in members
        assert load_toml(ROOT / crate_dir / "Cargo.toml")["package"]["name"] == package

    # TSF の DLL は aiokey.dll になる
    tsf = load_toml(ROOT / "crates/aiokey-tsf/Cargo.toml")
    assert tsf["lib"]["name"] == "aiokey"

    globals_rs = (ROOT / "crates/aiokey-tsf/src/globals.rs").read_text(encoding="utf-8")
    assert 'SERVICE_NAME: &str = "AI-O Key";' in globals_rs
    for prefix in UPSTREAM_GUID_PREFIXES:
        assert prefix not in globals_rs.lower()

    offenders = []
    for top in CODE_DIRS:
        for path in (ROOT / top).rglob("*"):
            if path.suffix not in CODE_SUFFIXES or "target" in path.parts:
                continue
            for no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                # 出典として URL を書く行は残してよい
                if "azookey" in line.lower() and "github.com" not in line.lower():
                    offenders.append(f"{path.relative_to(ROOT)}:{no}: {line.strip()}")
    assert offenders == []


def test_notice_and_spec_are_present():
    notice = (ROOT / "NOTICE").read_text(encoding="utf-8")
    for needle in ["azooKey-Windows", "MIT", "Mozc", "BSD-3-Clause", "zenz", "CC-BY-SA 4.0", "Wikipedia"]:
        assert needle in notice, needle

    spec = (ROOT / "docs/spec.md").read_text(encoding="utf-8")
    assert "AI-O Key" in spec
    assert "## 仕様からの変更点" in spec
