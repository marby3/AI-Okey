"""Issue #14 の受け入れテスト（基準 1〜4）：登録、インストーラー、実機確認の手順。

実機での確認と計測（基準 3〜5 の結果）は、v1.0.0-rc のインストーラーでユーザーが行い、docs/manual-test.md に記録する。
基準 5（AVX2 でのビルドと段2の時間の計測）は crates/aiokey-cli/tests/bench_stage2.rs で確かめる。
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_register_script_registers_and_unregisters_x64_and_x86():
    script = (ROOT / "scripts/register.ps1").read_text(encoding="utf-8")
    # x64 はふつうの regsvr32、x86 は SysWOW64 の regsvr32 で登録し、/u で解除する
    assert "System32\\regsvr32.exe" in script and "SysWOW64\\regsvr32.exe" in script
    assert "x86\\aiokey.dll" in script and "/u" in script
    assert "[switch]$Unregister" in script


def test_release_builds_and_attaches_the_installer():
    iss = (ROOT / "installer/Installer.iss").read_text(encoding="utf-8")
    for name in ["aiokey-server.exe", "aiokey-ui.exe", "aiokey-launcher.exe", "aiokey.dic",
                 "aiokey-stage2-q8_0.gguf", "aiokey-stage2.json"]:
        assert name in iss, name
    assert "AI-O Key Startup" in iss  # ログオン時の自動起動（タスク スケジューラ）
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert "ISCC" in release and "installer/Installer.iss" in release
    create = release[release.index("gh release create") :].split("\n\n")[0]
    assert "aiokey-setup-" in create


def test_manual_test_checklist_covers_typing_window_mode_and_stage2():
    doc = (ROOT / "docs/manual-test.md").read_text(encoding="utf-8")
    for item in ["htsthqtun", "果たして本当に", "kik", "keyboard", "/hqt", "Shift+Space",
                 "候補ウィンドウ", "数字キー", "モード表示", "変換キー", "段2"]:
        assert item in doc, item


def test_manual_test_explains_how_to_measure_memory_and_cpu():
    doc = (ROOT / "docs/manual-test.md").read_text(encoding="utf-8")
    for item in ["aiokey-server", "aiokey-ui", "WorkingSet", "CPU", "150", "20"]:
        assert item in doc, item
