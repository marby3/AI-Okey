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


def test_launcher_starts_at_logon_from_the_users_run_key_and_right_after_install():
    # rc.2：タスク スケジューラのタスクが実機で登録されていなかった。ランチャーは管理者でなくても動く
    # （候補ウィンドウも UIAccess を得られた）ので、管理者の権限の要らないユーザーの Run キーで起動する
    iss = (ROOT / "installer/Installer.iss").read_text(encoding="utf-8")
    run_key = [l for l in iss.splitlines() if "CurrentVersion\\Run" in l]
    assert run_key and "HKCU" in run_key[0] and "uninsdeletevalue" in run_key[0]
    assert '"""{app}\\aiokey-launcher.exe"""' in run_key[0]
    assert "/Create" not in iss and not (ROOT / "installer/AI-O Key Startup.xml").exists()
    # インストールの直後に、管理者ではないユーザーとして起動する。rc.2 までのタスクは消す
    launch = [l for l in iss.splitlines() if l.startswith('Filename: "{app}\\aiokey-launcher.exe"')]
    assert launch and "runasoriginaluser" in launch[0] and "nowait" in launch[0]
    assert '/Delete /TN ""AI-O Key Startup"" /F' in iss.split("[UninstallRun]")[0]


def test_ime_starts_the_launcher_when_the_server_is_not_running():
    # 自動起動に失敗していても、最初のキー入力でランチャーを起動する（アプリのジョブから切り離す）
    state = (ROOT / "crates/aiokey-tsf/src/engine/state.rs").read_text(encoding="utf-8")
    assert "aiokey-launcher.exe" in state and "CREATE_BREAKAWAY_FROM_JOB" in state
    # 候補ウィンドウは、UIAccess を得られなくても動き続ける
    app = (ROOT / "crates/aiokey-ui/src/app.rs").read_text(encoding="utf-8")
    assert "prepare_uiaccess_token()?" not in app


def test_tsf_activates_without_the_server_and_connects_on_the_first_key():
    # rc.1：Activate は、サーバーにつながらないとキーのシンクを登録せずに抜け、つながっても空の
    # append_text（サーバーが拒む）で失敗していた。サーバーが後から起動したときにもつなぎ直さなかった
    tip = (ROOT / "crates/aiokey-tsf/src/tsf/text_input_proccesor.rs").read_text(encoding="utf-8")
    activate = tip[tip.index("fn Activate") : tip.index("fn Deactivate")]
    assert 'append_text("".to_string())' not in activate
    assert "return Ok(());" not in activate
    composition = (ROOT / "crates/aiokey-tsf/src/engine/composition.rs").read_text(encoding="utf-8")
    assert "IMEState::connect()" in composition
    assert '.context("ipc_service is None")' not in composition


def test_register_script_has_a_utf8_bom_for_windows_powershell():
    # Windows PowerShell 5.1 は BOM のない .ps1 を ANSI（日本語版は Shift_JIS）で読むので、日本語が化ける
    assert (ROOT / "scripts/register.ps1").read_bytes().startswith(b"\xef\xbb\xbf")
