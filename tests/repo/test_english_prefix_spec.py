"""Issue #47 基準 4：仕様の英語の指定の説明が、文の途中の `/`（「・」）に合っている。"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_spec_explains_the_middle_dot_and_where_an_english_word_ends():
    spec = (ROOT / "docs/spec.md").read_text(encoding="utf-8")
    line = next(l for l in spec.splitlines() if l.startswith("- 明示指定"))
    for item in ["・", "zyq/F/knd", "ジョン・F・ケネディ", "次の /"]:
        assert item in line, item
