"""Issue #13 の受け入れテスト（基準 1〜4）：GPU サーバーで学習する前に、手元で済ませること。

基準 5〜9 は GPU サーバーでの学習の結果なので、計測して docs/eval/ に記録する。
"""

import json
import math
import subprocess
import sys
from pathlib import Path

import pytest
import torch

from aiokey import train
from test_train import BASE, DEV, ROOT, TRAIN, write_jsonl


def test_lm_head_runs_only_on_candidate_positions_with_the_same_log_probs():
    model, tokenizer = train.load(BASE, "cpu")
    model.eval()
    batch = [train.encode(tokenizer, e, max_candidates=32) for e in TRAIN]
    rows = []
    hook = model.lm_head.register_forward_hook(lambda _m, inputs, _o: rows.append(inputs[0].shape))
    with torch.no_grad():
        got = train.candidate_log_probs(model, batch)
    hook.remove()
    # 語彙ぶんの確率（lm_head）は、候補と EOS のトークンの位置だけで計算する
    counted = sum(len(s) - e.start for e in batch for s in e.sequences)
    assert sum(math.prod(shape[:-1]) for shape in rows) == counted

    # 全位置で計算する素朴な方法と同じ log P になる
    for e, lp in zip(batch, got):
        for seq, value in zip(e.sequences, lp.tolist()):
            with torch.no_grad():
                full = torch.log_softmax(model(torch.tensor([seq])).logits[0], dim=-1)
            expected = sum(full[p - 1, seq[p]].item() for p in range(e.start, len(seq)))
            assert value == pytest.approx(expected, abs=1e-3)


def run_train(tmp, *extra):
    tmp.mkdir(parents=True, exist_ok=True)
    out = tmp / "out"
    cmd = [
        sys.executable, "-m", "aiokey", "train",
        "--train", str(write_jsonl(tmp / "train.jsonl", TRAIN)),
        "--dev", str(write_jsonl(tmp / "dev.jsonl", DEV)),
        "--base", str(BASE),
        "--out", str(out),
        "--device", "cpu",
        "--batch-size", "1",
        "--epochs", "2",
        *extra,
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    return json.loads((out / "train.json").read_text())


def test_training_resumes_from_a_checkpoint(tmp_path):
    whole = run_train(tmp_path / "whole")
    checkpoint = tmp_path / "checkpoint"
    stopped = run_train(tmp_path / "part1", "--checkpoint", str(checkpoint), "--stop-after", "3")
    assert stopped["steps"] == 3 and stopped["stopped"]
    resumed = run_train(tmp_path / "part2", "--checkpoint", str(checkpoint), "--resume")
    # 4 例 × 2 エポック = 8 ステップ。止めたところ（3）から続け、学習率の予定も続きから進む
    assert resumed["resumed_from"] == 3
    assert resumed["steps"] == whole["steps"] == 8
    assert resumed["final_lr"] == pytest.approx(whole["final_lr"])
    # 順番・乱数・最適化の状態も引き継ぐので、止めずに最後まで学習したのと同じ結果になる
    assert resumed["dev"]["decision_loss"] == pytest.approx(whole["dev"]["decision_loss"], abs=1e-4)


def test_small_training_lowers_the_dev_decision_loss(tmp_path):
    # 学習に使った例を dev にもして、学習で decision loss が下がることを確かめる
    out = tmp_path / "out"
    data = write_jsonl(tmp_path / "train.jsonl", TRAIN)
    cmd = [
        sys.executable, "-m", "aiokey", "train",
        "--train", str(data), "--dev", str(data), "--base", str(BASE), "--out", str(out),
        "--device", "cpu", "--batch-size", "2", "--epochs", "3", "--lr", "1e-4", "--warmup", "0",
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    report = json.loads((out / "train.json").read_text())
    assert report["dev"]["decision_loss"] < report["dev_before"]["decision_loss"]


def test_gpu_server_procedure_is_documented():
    doc = (ROOT / "docs/training-gpu.md").read_text(encoding="utf-8")
    for needle in ["uv sync", "--extra cuda", "python -m aiokey train", "--resume", "--checkpoint", "tar -xf"]:
        assert needle in doc, needle
