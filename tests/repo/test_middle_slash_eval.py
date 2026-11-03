"""Issue #52 基準 4：文の途中の `/` のあとが長い入力について、直す前とあとの A・B と段1の時間の記録。"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_middle_slash_results_are_recorded_before_and_after_the_fix():
    text = (ROOT / "docs/eval/stage1.md").read_text(encoding="utf-8")
    start = text.index("## 文の途中の `/` のあとが長い入力")
    end = text.find("\n## ", start + 1)
    s = text[start : end if end != -1 else len(text)]
    for item in ["ATC 2026", "子音だけ", "母音あり", "47", "全体", "直す前", "直したあと",
                 "完全一致", "1 ページ目", "p50", "p95"]:
        assert item in s, item
    assert len(re.findall(r"\d+(\.\d+)?%", s)) >= 16
    assert len(re.findall(r"\d+(\.\d+)? ?ms", s)) >= 2
