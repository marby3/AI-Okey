"""Issue #55 の受け入れテスト：段2を学習し直した記録（docs/eval/stage2-v3.md）と、モデルを入れ替えるかの判断。"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOC = ROOT / "docs/eval/stage2-v3.md"
sys.path.insert(0, str(ROOT / "python/src"))


def section(heading):
    text = DOC.read_text(encoding="utf-8")
    start = text.index(f"## {heading}")
    end = text.find("\n## ", start + 1)
    return text[start : end if end >= 0 else len(text)]


def rows(text):
    """Markdown の表の本体の行を、セルのリストにして返す。"""
    out = []
    for line in text.splitlines():
        if line.startswith("|") and not re.match(r"^\|[\s:|-]+\|$", line):
            out.append([c.strip().strip("*") for c in line.strip().strip("|").split("|")])
    return out[1:]


def percent(cell):
    return float(re.search(r"([\d.]+)%", cell).group(1))


def test_learning_curve_is_recorded_for_every_stage():
    # 基準 1：段階ごとの dev の top-1、decision loss、学習の時間。伸びが止まる前にユーザーの判断で止めたことも記録する
    table = rows(section("データの量ごとの伸び"))
    assert [r[0] for r in table] == ["20,000", "40,000", "80,000", "160,000"]
    tops = [percent(r[1]) for r in table]
    assert tops == sorted(tops)
    for r in table:
        assert re.fullmatch(r"\d+\.\d+", r[2]), r  # decision loss
        assert re.search(r"\d+(\.\d+)? 時間", r[4]), r  # 学習の時間
    text = section("データの量ごとの伸び")
    assert "--base" in text and "ユーザー" in text


def test_best_model_is_compared_with_the_current_model():
    # 基準 2：test セットと ATC 2026（句ごと、子音だけと母音あり）の A と B
    table = rows(section("今のモデルとの比較"))
    names = [r[0] for r in table]
    for name in ["ATC 2026 句・子音だけ", "ATC 2026 句・母音あり", "test 句・子音だけ", "test 句・母音あり"]:
        assert name in names, name
    for r in table:
        assert all(percent(c) >= 0 for c in r[1:5]), r


def test_context_difference_is_recorded_and_can_be_measured(tmp_path):
    # 基準 3：文脈あり・なしの A と B。文脈なしの測定は段2に空の文脈を渡す
    from aiokey.analyze import Analyzer

    table = rows(section("文脈のあり・なし"))
    assert "ATC 2026 句・子音だけ" in [r[0] for r in table]
    for r in table:
        assert all(percent(c) >= 0 for c in r[1:5]), r

    class FakeConverter:
        def convert_with_costs(self, input_, k):
            return [("候補一", "こうほいち", 0), ("候補二", "こうほに", 1)]

    class FakeScorer:
        def __init__(self):
            self.contexts = []

        def score(self, context, input_, candidates):
            self.contexts.append(context)
            return [0.5] * len(candidates), None

    example = {"context": "前の文。", "input": "khi", "target": "候補一"}
    scorer = FakeScorer()
    Analyzer(FakeConverter(), scorer).rank(example)
    Analyzer(FakeConverter(), scorer, use_context=False).rank(example)
    assert scorer.contexts == ["前の文。", ""]


def test_shipped_model_follows_the_decision_rule():
    # 基準 4：ATC 2026 の句・子音だけで B が上がり A が下がらなければ入れ替える。そうでなければ入れ替えず、理由を記録する
    row = next(r for r in rows(section("今のモデルとの比較")) if r[0] == "ATC 2026 句・子音だけ")
    a_before, a_after, b_before, b_after = (percent(c) for c in row[1:5])
    decision = section("判断")
    script = (ROOT / "scripts/fetch-stage2-model.sh").read_text(encoding="utf-8")
    if b_after > b_before and a_after >= a_before:
        assert "入れ替える" in decision
        assert re.search(r'^MODEL_TAG="stage2-model-v2"$', script, re.M)
    else:
        assert "入れ替えない" in decision and "理由" in decision
        assert re.search(r'^MODEL_TAG="stage2-model-v1"$', script, re.M)
