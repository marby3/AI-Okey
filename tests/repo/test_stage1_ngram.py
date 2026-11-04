"""Issue #58 の受け入れテスト：段1の並べ替えの記録（docs/eval/stage1-ngram.md）と配布。"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs/eval/stage1-ngram.md"
SCRIPT = ROOT / "scripts/fetch-ngram.sh"


def section(heading):
    text = DOC.read_text(encoding="utf-8")
    start = text.index(f"## {heading}")
    end = text.find("\n## ", start + 1)
    return text[start : end if end >= 0 else len(text)]


def rows(text):
    """節の最初の Markdown の表の本体の行を、セルのリストにして返す。"""
    out = []
    for line in text.splitlines():
        if line.startswith("|"):
            if not re.match(r"^\|[\s:|-]+\|$", line):
                out.append([c.strip().strip("*") for c in line.strip().strip("|").split("|")])
        elif out:
            break
    return out[1:]


def percent(cell):
    return float(re.search(r"([\d.]+)%", cell).group(1))


def test_settings_are_chosen_on_dev_only():
    # 基準 3：N、重み、足切り、枠を dev の例だけで選び、選んだ過程を記録する
    text = section("設定の決め方")
    for item in ["dev", "N", "重み", "足切り", "MAX_ENTRIES_PER_SPAN", "skip_cost_gap"]:
        assert item in text, item
    assert {"64", "128", "256"} <= {r[0] for r in rows(text)}
    assert "選び直さない" in text


def test_before_and_after_are_compared_for_every_set_with_stage2():
    # 基準 5：同じコードの表のあり・なしを、段2（stage2-model-v2、候補 8 件）を含めて比べる
    text = section("前後の比較")
    assert "stage2-model-v2" in text and "作り直さず" in text
    table = rows(text)
    names = [r[0] for r in table]
    for corpus in ["ATC 2026", "test", "固有名詞"]:
        for unit in ["句", "文"]:
            for keys in ["子音だけ", "母音あり"]:
                assert f"{corpus} {unit}・{keys}" in names, (corpus, unit, keys)
    for r in table:
        # A、B、候補 32 件、文字の一致率のそれぞれのなし・あり
        assert all("%" in c for c in r[1:9]), r
        assert re.fullmatch(r"\d[\d,]*／\d[\d,]*", r[9]), r  # 勝ち／負け
    stage2 = rows(section("段2を飛ばす割合と候補ウィンドウ"))
    assert stage2 and all("%" in c for r in stage2 for c in r[1:]), stage2


def test_latency_and_memory_are_recorded_for_each_pool_size():
    # 基準 6：表のあり・なしと N ごとの遅延、サーバーのメモリ、ファイルの大きさ
    text = section("遅延とメモリ")
    table = rows(text)
    labels = [r[0] for r in table]
    assert "なし" in labels and {"64", "128", "256"} <= set(labels)
    for r in table:
        assert all(re.search(r"\d", c) for c in r[1:]), r
    header = next(l for l in text.splitlines() if l.startswith("|"))
    for item in ["段1", "段1＋段2", "p50", "p95", "起動直後", "ピーク"]:
        assert item in header, item
    assert re.search(r"表.*\d+(\.\d+)? MB", text) and re.search(r"辞書.*\d+(\.\d+)? MB", text)


def test_default_follows_the_decision_rule():
    # 基準 7：ATC 2026 の句・子音だけで B が上がり A が下がらなければ既定で有効。そうでなければ無効のまま理由を記録する
    row = next(r for r in rows(section("前後の比較")) if r[0] == "ATC 2026 句・子音だけ")
    a_before, a_after, b_before, b_after = (percent(c) for c in row[1:5])
    decision = section("判断")
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    if b_after > b_before and a_after >= a_before:
        assert "既定で有効" in decision and "scripts/fetch-ngram.sh" in release
    else:
        assert "既定で無効" in decision and "scripts/fetch-ngram.sh" not in release


def test_table_ships_pinned_and_corpus_text_stays_out():
    # 基準 8：表の版と SHA-256 を固定して配布物に入れる。コーパスの本文と学習の例は入れず、由来を書く
    script = SCRIPT.read_text(encoding="utf-8")
    assert re.search(r'^TABLE_TAG="stage1-ngram-v\d+"$', script, re.M)
    assert re.search(r'^TABLE_SHA256="[0-9a-f]{64}"$', script, re.M)
    assert "sha256sum -c" in script
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    package = release[release.index("name: Package") :]
    assert "aiokey-ngram.bin" in package
    iss = (ROOT / "installer/Installer.iss").read_text(encoding="utf-8")
    assert 'Source: "{#SourceDir}/aiokey-ngram.bin"' in iss and "DeleteOldFile('aiokey-ngram.bin')" in iss
    # 表はリポジトリに入れない
    assert subprocess.run(["git", "-c", "safe.directory=*", "check-ignore", "-q", "target/ngram/aiokey-ngram.bin"],
                          cwd=ROOT).returncode == 0
    # 表の形式は 3 文字までしか表せず、4 文字の並びは書けない
    engine = (ROOT / "crates/aiokey-engine/src/ngram.rs").read_text(encoding="utf-8")
    assert "MAX_CHARS: usize = 3" in engine
    notice = (ROOT / "NOTICE").read_text(encoding="utf-8")
    assert "aiokey-ngram.bin" in notice
    data = (ROOT / "docs/training-data.md").read_text(encoding="utf-8")
    start = data.index("## 段1の文字 n-gram の表")
    table = data[start : data.find("\n## ", start + 1)]
    for item in ["Wikipedia", "Wikinews", "Tatoeba", "Common Voice", "青空文庫", "Web", "使わない", "CC BY-SA", "python -m aiokey ngram"]:
        assert item in table, item


def test_tampered_table_fails_verification(tmp_path):
    (tmp_path / "aiokey-ngram.bin").write_bytes(b"tampered")
    result = subprocess.run(["bash", str(SCRIPT), "--verify-only", str(tmp_path)], capture_output=True, text=True)
    assert result.returncode != 0
    assert "aiokey-ngram" in result.stdout + result.stderr


def test_investigation_and_rejected_methods_are_recorded():
    # 基準 9：順位の分布、作れない例の内訳、採らなかった方法の数字と理由、計測の手順
    assert rows(section("正解の順位"))
    assert "MAX_ENTRIES_PER_SPAN" in section("語の並びで作れない例")
    rejected = section("採らなかった方法")
    for item in ["K を", "SudachiDict", "頻度"]:
        assert item in rejected, item
    procedure = section("計測の手順")
    assert "python -m aiokey ngram" in procedure and "python -m aiokey analyze" in procedure
