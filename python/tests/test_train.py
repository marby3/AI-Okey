"""Issue #9 の受け入れテスト：段2の学習パイプライン。

学習の元にする zenz（Hugging Face 形式）と llama.cpp の変換スクリプトは scripts/fetch-zenz.sh が用意する。
"""

import json
import math
import os
import subprocess
import sys
from pathlib import Path

import pytest
import torch

import aiokey
from aiokey import train

ROOT = Path(__file__).resolve().parents[2]
BASE = Path(os.environ.get("AIOKEY_ZENZ_DIR", ROOT / "data/zenz-v2.5-xsmall"))


def example(context, input_, target, candidates):
    return {"context": context, "input": input_, "target": target, "candidates": candidates}


TRAIN = [
    example("", "kik", "開花", ["機器", "開花", "危機"]),
    example("桜が", "skmsht", "咲きました", ["咲きました", "先ました"]),
    example("", "htsthqtun", "果たして本当に", ["果たして本当に", "果たしてほんとうに"]),
    example("今日は", "tnkgy", "天気が良い", ["天気が良い", "転機が良い", "天気がよい"]),
]
DEV = [
    example("", "kk", "仮名", ["仮名", "金", "家内"]),
    example("雨が", "frms", "降ります", ["振ります", "降ります"]),
]


def write_jsonl(path, rows):
    path.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows), encoding="utf-8")
    return path


def test_decision_loss_matches_hand_computation():
    # log P = [-1, -2, -3]。log softmax の分母は log(e^-1 + e^-2 + e^-3) = -1 + log(1 + e^-1 + e^-2)
    log_probs = torch.tensor([-1.0, -2.0, -3.0])
    log_z = -1 + math.log(1 + math.exp(-1) + math.exp(-2))
    assert train.decision_loss(log_probs, 0).item() == pytest.approx(-(-1 - log_z), abs=1e-6)
    assert train.decision_loss(log_probs, 2).item() == pytest.approx(-(-3 - log_z), abs=1e-6)
    assert train.decision_loss(log_probs, 0).item() == pytest.approx(0.407606, abs=1e-6)


def test_lm_weight_mixes_lm_loss_and_zero_means_decision_only():
    model, tokenizer = train.load(BASE, "cpu")
    model.eval()
    batch = [train.encode(tokenizer, e, max_candidates=32) for e in TRAIN]
    with torch.no_grad():
        total0, decision0, lm0 = train.losses(model, batch, lm_weight=0.0)
        total5, decision5, lm5 = train.losses(model, batch, lm_weight=0.5)
    assert lm0.item() > 0
    assert total0.item() == decision0.item()
    assert total5.item() == pytest.approx(decision5.item() + 0.5 * lm5.item(), rel=1e-6)
    # 混合比はコマンドから設定できる
    args = train.parser().parse_args(["--train", "a", "--dev", "b", "--out", "c", "--lm-weight", "0"])
    assert args.lm_weight == 0.0


@pytest.fixture(scope="module")
def trained(tmp_path_factory):
    """小さなデータで、学習・温度スケーリング・GGUF の書き出しを 1 コマンドで通す。"""
    tmp = tmp_path_factory.mktemp("train")
    out = tmp / "out"
    cmd = [
        sys.executable, "-m", "aiokey", "train",
        "--train", str(write_jsonl(tmp / "train.jsonl", TRAIN)),
        "--dev", str(write_jsonl(tmp / "dev.jsonl", DEV)),
        "--base", str(BASE),
        "--out", str(out),
        "--epochs", "1",
        "--f32",
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    return out


def test_one_command_trains_calibrates_and_exports_q8_0_on_cpu(trained):
    report = json.loads((trained / "train.json").read_text())
    assert report["device"] == "cpu"
    assert report["steps"] >= 1
    assert report["temperature"] > 0
    gguf = trained / "aiokey-stage2-q8_0.gguf"
    assert gguf.read_bytes()[:4] == b"GGUF"
    # 書き出した GGUF は採点器で開け、温度を渡して使える
    scorer = aiokey.Scorer(str(gguf), "aiokey", 2, 32, report["temperature"])
    probs, _margin = scorer.score("", "kik", [("機器", ""), ("開花", "")])
    assert sum(probs) == pytest.approx(1.0, abs=1e-5)


def test_exported_gguf_log_probs_match_pytorch(trained):
    model, tokenizer = train.load(trained / "hf", "cpu")
    model.eval()
    # 許容誤差は候補のトークン 1 つあたり。誤差はトークンごとに積み上がる。
    # f32 でも ggml の GELU は fp16 の表で近似するので少しずれる（実測で最大 0.005）。Q8_0 は量子化の分が加わる（最大 0.11）
    for name, per_token in [("aiokey-stage2-f32.gguf", 0.02), ("aiokey-stage2-q8_0.gguf", 0.25)]:
        scorer = aiokey.Scorer(str(trained / name), "aiokey", 2, 32)
        for e in DEV:
            encoded = train.encode(tokenizer, e, max_candidates=32)
            with torch.no_grad():
                expected = train.candidate_log_probs(model, [encoded])[0].tolist()
            got = scorer.log_probs(e["context"], e["input"], [(c, "") for c in e["candidates"]])
            assert len(got) == len(expected), name
            for g, x, seq in zip(got, expected, encoded.sequences):
                assert g == pytest.approx(x, abs=per_token * (len(seq) - encoded.start)), name


def test_train_image_uses_cuda_and_same_command_switches_device(monkeypatch):
    # 同じコマンド（--device の既定は auto）で、GPU があれば CUDA、なければ CPU で学習する
    assert train.parser().parse_args(["--train", "a", "--dev", "b", "--out", "c"]).device == "auto"
    monkeypatch.setattr(torch.cuda, "is_available", lambda: True)
    assert train.resolve_device("auto") == "cuda"
    monkeypatch.setattr(torch.cuda, "is_available", lambda: False)
    assert train.resolve_device("auto") == "cpu"
    # GPU 用のイメージは CUDA 版の torch を入れ、同じ `python -m aiokey` を入口にする
    # （イメージのビルドは Docker のあるホストで `docker build -f docker/train.Dockerfile .` で確かめる）
    dockerfile = (ROOT / "docker/train.Dockerfile").read_text()
    assert "--extra cuda" in dockerfile
    assert 'ENTRYPOINT ["python", "-m", "aiokey"]' in dockerfile
