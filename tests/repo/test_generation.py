"""Issue #40 の受け入れテスト（基準 2〜4）：段2で文を生成する方式の比較、速さ、判断の記録。

結果は docs/eval/generation.md に記録する（数字は python -m aiokey compare で出す）。
基準 1 は python/tests/test_generate.py で確かめる。
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs/eval/generation.md"
PERCENT = re.compile(r"\d+(\.\d+)?%")


def section(title: str) -> str:
    text = DOC.read_text(encoding="utf-8")
    start = text.index(f"## {title}")
    end = text.find("\n## ", start + 1)
    return text[start : end if end != -1 else len(text)]


def test_methods_are_compared_on_the_same_examples_as_issue_39():
    # 基準 2
    s = section("方式の比較")
    for item in ["今の方式", "生成だけ", "合わせた", "ATC 2026", "子音だけ", "母音あり", "固有名詞",
                 "完全一致", "1 ページ目", "候補に正解がある"]:
        assert item in s, item
    assert len(PERCENT.findall(s)) >= 18


def test_latency_p50_and_p95_are_recorded_next_to_the_current_method():
    # 基準 3
    s = section("速さ")
    for item in ["p50", "p95", "今の方式", "生成"]:
        assert item in s, item
    assert len(re.findall(r"\d+(\.\d+)? ?ms", s)) >= 4


def test_decision_on_shipping_is_recorded():
    # 基準 4
    s = section("判断")
    assert "本番" in s
