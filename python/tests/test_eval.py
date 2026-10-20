"""Issue #7 の受け入れテスト：評価ハーネス。

基準 4（aiokey-cli bench が p50 と p95 を出す）は、#4 の
crates/aiokey-cli/tests/convert.rs::bench_reports_p50_and_p95 で確かめている。
"""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

import aiokey
from aiokey import evaluate

ROOT = Path(__file__).resolve().parents[2]
MOZC = Path(os.environ.get("AIOKEY_MOZC_DIR", ROOT / "data/mozc"))


@pytest.fixture(scope="module")
def dict_path(tmp_path_factory):
    path = tmp_path_factory.mktemp("dict") / "aiokey.dic"
    aiokey.build_dictionary(str(MOZC), str(path))
    return path


def example(input_, target, reading, english=False):
    return {
        "input": input_,
        "target": target,
        "reading": reading,
        "tokens": [{"surface": target, "reading": reading, "english": english}],
    }


EXAMPLES = [
    example("htsthqtun", "果たして本当に", "はたしてほんとうに"),
    example("kik", "開花", "かいか"),
    example("knkeyboardds", "このkeyboardです", "このkeyboardです", english=True),
    example("zzzz", "ぬぽぽぺ", "ぬぽぽぺ"),
]


def test_eval_reports_recall_and_top1_top5(dict_path, tmp_path):
    # 3 位に来る候補を正解にした例を足し、top-5 が top-1 とも再現率とも違う値になるようにする
    third = aiokey.Converter(str(dict_path)).convert("kik", 5)[2]
    examples = EXAMPLES + [example("kik", third, "きき")]
    path = tmp_path / "test.jsonl"
    path.write_text("".join(json.dumps(e, ensure_ascii=False) + "\n" for e in examples))
    out = subprocess.run(
        [sys.executable, "-m", "aiokey", "eval", "--examples", str(path), "--dict", str(dict_path), "-k", "32"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    report = json.loads(out)
    assert report["examples"] == 5
    # 果たして本当に・このkeyboardです は 1 位、3 位の候補は 5 位以内、開花 は 6〜32 位、ぬぽぽぺ は入らない
    assert report["recall_at_k"] == pytest.approx(4 / 5)
    assert report["top1"] == pytest.approx(2 / 5)
    assert report["top5"] == pytest.approx(3 / 5)


def test_keystroke_reduction_against_romaji():
    # は た し て ほ ん と う に = 2+2+2+2+2+1+2+1+2 = 16 打鍵
    # （ん は子音の前なら n の 1 打鍵、母音・や行・な行の前と語末では nn の 2 打鍵）
    assert evaluate.romaji_keys("はたしてほんとうに") == 16
    assert evaluate.romaji_keys("こんや") == 2 + 2 + 2
    # っ は次の子音を重ねて 1、ゃ は ky+a で 1 足す、ー は 1、英字はそのまま
    assert evaluate.romaji_keys("まって") == 2 + 1 + 2
    assert evaluate.romaji_keys("きょう") == 3 + 1
    assert evaluate.romaji_keys("らーめん") == 2 + 1 + 2 + 2
    assert evaluate.romaji_keys("keyboard") == 8
    report = evaluate.summarize(
        [{**EXAMPLES[0], "candidates": ["果たして本当に"]}], k=32
    )
    assert report["keystroke_reduction"] == pytest.approx(1 - 9 / 16)


def test_english_subset_top1_is_reported_separately():
    examples = [
        {**EXAMPLES[0], "candidates": ["果たして本当に"]},
        {**EXAMPLES[2], "candidates": ["このkeyboardでし", "このkeyboardです"]},
    ]
    report = evaluate.summarize(examples, k=32)
    assert report["top1"] == pytest.approx(1 / 2)
    assert report["english_examples"] == 1
    assert report["english_top1"] == pytest.approx(0.0)


def test_stage1_baseline_is_recorded():
    doc = (ROOT / "docs/eval/stage1-baseline.md").read_text(encoding="utf-8")
    for needle in ["再現率@32", "top-1", "top-5", "打鍵削減率", "英単語を含む文", "p50", "p95", "jawiki-20261001"]:
        assert needle in doc, needle


def test_broken_examples_file_names_the_line(tmp_path):
    # 評価データは外で作るファイル。壊れた行があれば、行番号つきでエラーにする
    path = tmp_path / "broken.jsonl"
    path.write_text(json.dumps(EXAMPLES[0], ensure_ascii=False) + "\n{not json\n")
    with pytest.raises(evaluate.ExamplesError, match=":2"):
        list(evaluate.read_examples(path))
    for line in [json.dumps({"input": "k"}), "null", "[1, 2]"]:
        path.write_text(line + "\n")
        with pytest.raises(evaluate.ExamplesError, match=":1"):
            list(evaluate.read_examples(path))
