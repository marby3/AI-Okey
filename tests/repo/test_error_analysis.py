"""Issue #39 の受け入れテスト：変換の誤りの分析と、正解率の目標の測り方。

分析の結果は docs/eval/errors.md に記録する（数字は python -m aiokey analyze で出す）。
"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs/eval/errors.md"
SCRIPT = ROOT / "scripts/fetch-atc.sh"
SUMS = ROOT / "scripts/atc.sha256"
PERCENT = re.compile(r"\d+(\.\d+)?%")


def section(title: str) -> str:
    text = DOC.read_text(encoding="utf-8")
    start = text.index(f"## {title}")
    end = text.find("\n## ", start + 1)
    return text[start : end if end != -1 else len(text)]


def test_errors_are_split_by_whether_the_answer_is_in_stage1_and_by_cause():
    # 基準 1
    s = section("誤りの内訳")
    for item in ["候補に正解がない", "候補にあるが 1 位でない", "辞書にない語", "固有名詞・カタカナ語",
                 "数字", "英字", "文の長さ", "読みの違い", "表記の違い"]:
        assert item in s, item
    assert len(PERCENT.findall(s)) >= 10


def test_real_typing_sets_are_fetched_or_committed_and_readings_are_checked():
    # 基準 2：ATC のお題は取得して検証する（本文はコミットしない）。固有名詞の文はコミットする
    script = SCRIPT.read_text(encoding="utf-8")
    assert "https://o24.works/atc/atc26.txt" in script and "sha256sum" in script
    assert re.fullmatch(r"[0-9a-f]{64}  atc26\.txt\n", SUMS.read_text())
    proper = (ROOT / "docs/eval/sets/proper-nouns.txt").read_text(encoding="utf-8").splitlines()
    assert len([l for l in proper if l.strip()]) >= 30 and "ジョン・F・ケネディ" in "".join(proper)
    s = section("実際に打つ文")
    for item in ["ATC 2026", "固有名詞", "読みの誤り"]:
        assert item in s, item
    assert PERCENT.search(s)


def test_fetch_atc_rejects_a_file_that_does_not_match_the_pinned_hash(tmp_path):
    # 信頼の境界：取ってきたお題が固定した版と違えば、使わずに失敗する
    assert SCRIPT.is_file()
    (tmp_path / "atc26.txt").write_text("tampered\n", encoding="utf-8")
    result = subprocess.run(["bash", str(SCRIPT), "--verify-only", "--dir", str(tmp_path)],
                            capture_output=True, text=True)
    assert result.returncode != 0 and "FAILED" in result.stdout + result.stderr


def test_measures_are_compared_on_the_test_set_and_the_real_typing_sets():
    # 基準 3
    s = section("測り方の比較")
    for item in ["完全一致", "句ごと", "1 ページ目", "母音", "test", "ATC 2026", "固有名詞"]:
        assert item in s, item
    assert len(PERCENT.findall(s)) >= 12


def test_share_of_inputs_that_cannot_be_decided_without_context_is_estimated():
    # 基準 4
    s = section("文脈がないと決められない入力")
    assert "見積もり" in s and PERCENT.search(s)
