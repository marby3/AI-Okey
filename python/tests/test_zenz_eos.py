"""Issue #26 の受け入れテスト：学習前の zenz の GGUF の EOS。

zenz の config.json は eos_token_id が 2（語彙では <s>）で、語彙の </s>（3）と食い違う。
scripts/fetch-zenz.sh が作る GGUF の EOS は </s> でなければならない。
"""

import json
import os
import sys
from pathlib import Path

import pytest
import torch

import aiokey
from aiokey import train

ROOT = Path(__file__).resolve().parents[2]
BASE = Path(os.environ.get("AIOKEY_ZENZ_DIR", ROOT / "data/zenz-v2.5-xsmall"))
MODELS = ROOT / "data/models"


def gguf_eos(path: Path) -> int:
    sys.path.insert(0, str(train.llama_cpp_dir() / "gguf-py"))
    import gguf

    field = gguf.GGUFReader(str(path)).fields["tokenizer.ggml.eos_token_id"]
    return int(field.parts[field.data[0]][0])


def test_fetched_gguf_uses_end_of_sequence_token_as_eos():
    vocab = json.loads((BASE / "vocab.json").read_text(encoding="utf-8"))
    assert vocab["</s>"] == 3
    for name in ["zenz-v2.5-xsmall-q8_0.gguf", "zenz-v2.5-xsmall-f32.gguf"]:
        assert gguf_eos(MODELS / name) == vocab["</s>"], name


CANDIDATES = [
    ("果たして本当に", "はたしてほんとうに"),
    ("浸して本当に", "ひたしてほんとうに"),
    ("雨が降ります", "あめがふります"),
]


def test_untrained_f32_gguf_log_probs_match_pytorch():
    context = "今日は"
    model, tokenizer = train.load(BASE, "cpu")
    model.eval()
    scorer = aiokey.Scorer(str(MODELS / "zenz-v2.5-xsmall-f32.gguf"), "zenz", 2, 32)
    got = scorer.log_probs(context, "", CANDIDATES)
    for (surface, reading), g in zip(CANDIDATES, got):
        prompt = tokenizer.encode(aiokey.prompt("zenz", context, "", reading), add_special_tokens=False)
        ids = prompt + tokenizer.encode(surface, add_special_tokens=False) + [tokenizer.convert_tokens_to_ids("</s>")]
        with torch.no_grad():
            log_probs = torch.log_softmax(model(torch.tensor([ids])).logits[0], dim=-1)
        expected = sum(log_probs[p - 1, ids[p]].item() for p in range(len(prompt), len(ids)))
        # #9 の基準 4 と同じく、候補のトークン 1 つあたり 0.02 まで
        assert g == pytest.approx(expected, abs=0.02 * (len(ids) - len(prompt))), surface
