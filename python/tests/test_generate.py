"""Issue #40 の受け入れテスト（基準 1）：段2で、打った入力に合う文を生成して候補を作る。

学習済みの段2（data/stage2/aiokey-stage2-q8_0.gguf、Release の配布物）が要る。ない環境では飛ばす。
基準 2〜4（比較・速さ・判断の記録）は tests/repo/test_generation.py で確かめる。
"""

import json
import os
from pathlib import Path

import pytest

import aiokey

ROOT = Path(__file__).resolve().parents[2]
MOZC = Path(os.environ.get("AIOKEY_MOZC_DIR", ROOT / "data/mozc"))
STAGE2 = Path(os.environ.get("AIOKEY_STAGE2_GGUF", ROOT / "data/stage2/aiokey-stage2-q8_0.gguf"))


@pytest.fixture(scope="module")
def dict_path(tmp_path_factory):
    path = tmp_path_factory.mktemp("dict") / "aiokey.dic"
    aiokey.build_dictionary(str(MOZC), str(path))
    return path


def test_generation_finds_an_answer_that_is_not_in_the_stage1_candidates(dict_path):
    # 基準 1：ジョン・F・ケネディは段1では 32 位以内に出ない（#47）
    if not STAGE2.is_file():
        pytest.skip(f"{STAGE2} がない（学習済みの段2は Release の配布物）")
    temperature = json.loads((STAGE2.parent / "aiokey-stage2.json").read_text(encoding="utf-8"))["temperature"]
    converter = aiokey.Converter(str(dict_path))
    scorer = aiokey.Scorer(str(STAGE2), "aiokey", 2, 64, temperature)
    target, input_ = "ジョン・F・ケネディ", "zyq/F/knd"
    assert target not in converter.convert(input_, 32)
    # #52 で閉じる `/` を語の終わりとしても読むようになり、重み 0.5 では幅 8 のビームが「・」のない並びに流れる。
    # 重み 0.25 は #40 の設定の探索で 0.5 と正解率が同じ（docs/eval/generation.md）
    generated = [surface for surface, _ in scorer.generate(converter, "", input_, cost_weight=0.25)]
    assert target in generated
