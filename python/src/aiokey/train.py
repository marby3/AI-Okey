"""段2の採点器（zenz-v2.5-xsmall）を学習する。

候補集合の上の log softmax の負の対数尤度（OpenJev の decision loss）で、モデルの全パラメータを学習する。
正解文の言語モデル損失（トークンあたりの負の対数尤度）を `--lm-weight` の割合で混ぜられる。
学習のあとは dev で温度スケーリングをして確信度を校正し、GGUF（Q8_0）に書き出す。

    python -m aiokey train --train data/examples/train.jsonl --dev data/examples/dev.jsonl --out build/stage2

GPU があれば CUDA で、なければ CPU で学習する（`--device` で固定もできる）。出力：

- `<out>/hf/`：学習したモデル（Hugging Face 形式）
- `<out>/aiokey-stage2-q8_0.gguf`：採点器（aiokey-scorer）で読む GGUF。`--f32` で f32 版も書く
- `<out>/train.json`：温度、損失、dev の top-1 などの記録

採点器と同じ値を学習するように、プロンプトは aiokey-scorer の `prompt`（aiokey 形式）で作り、
log P(候補 | 文脈, 入力) は「プロンプト + 候補 + EOS」のうち候補と EOS のトークンの log P の和とする。
"""

from __future__ import annotations

import argparse
import contextlib
import json
import math
import random
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import torch
import torch.nn.functional as F
from transformers import AutoTokenizer, GPT2LMHeadModel

import aiokey

ROOT = Path(__file__).resolve().parents[3]
# 元のモデルから学習後のモデルへ写すトークナイザーのファイル
TOKENIZER_FILES = ["vocab.json", "merges.txt", "tokenizer_config.json", "special_tokens_map.json"]


@dataclass
class Encoded:
    """1 例ぶんの候補のトークン列。"""

    sequences: list[list[int]]
    # 候補の部分が始まる位置（プロンプトの長さ。全候補で同じ）
    start: int
    gold: int


def decision_loss(log_probs: torch.Tensor, gold: int) -> torch.Tensor:
    """候補集合の上の log softmax に対する、正解の負の対数尤度。"""
    return -torch.log_softmax(log_probs, dim=-1)[gold]


def resolve_device(device: str) -> str:
    if device == "auto":
        return "cuda" if torch.cuda.is_available() else "cpu"
    if device == "cuda" and not torch.cuda.is_available():
        raise ValueError("--device cuda was given, but CUDA is not available")
    return device


def load(path: Path, device: str):
    """モデルとトークナイザーを読む（学習は f32 で行う）。"""
    tokenizer = AutoTokenizer.from_pretrained(str(path))
    model = GPT2LMHeadModel.from_pretrained(str(path), dtype=torch.float32).to(device)
    model.config.use_cache = False
    return model, tokenizer


def encode(tokenizer, example: dict, max_candidates: int, max_length: int = 1024) -> Encoded | None:
    """例を候補のトークン列にする。正解の列がモデルに入らなければ None。"""
    target = example["target"]
    candidates = list(dict.fromkeys(example["candidates"]))[:max_candidates]
    if target not in candidates:
        candidates = candidates[: max(max_candidates - 1, 0)] + [target]
    text = aiokey.prompt("aiokey", example["context"], example["input"], "")
    prompt = tokenizer.encode(text.replace("\0", ""), add_special_tokens=False)
    sequences, gold = [], None
    for c in candidates:
        seq = prompt + tokenizer.encode(c.replace("\0", ""), add_special_tokens=False) + [tokenizer.eos_token_id]
        # 長すぎる候補は落とす（採点器も、切り詰めずに採点しない）
        if len(seq) > max_length:
            continue
        if c == target:
            gold = len(sequences)
        sequences.append(seq)
    if gold is None:
        return None
    return Encoded(sequences, len(prompt), gold)


def candidate_log_probs(model, batch: list[Encoded]) -> list[torch.Tensor]:
    """例ごとに、候補の log P(候補 | 文脈, 入力) を並べたテンソルを返す。"""
    device = next(model.parameters()).device
    rows = [s for e in batch for s in e.sequences]
    width = max(len(s) for s in rows)
    ids = torch.zeros((len(rows), width), dtype=torch.long)
    attention = torch.zeros((len(rows), width), dtype=torch.long)
    # 位置 p のトークンは、位置 p-1 の出力から確率を取る。候補の部分（start 以降）だけを数える
    counted = torch.zeros((len(rows), width - 1), dtype=torch.bool)
    i = 0
    for e in batch:
        for s in e.sequences:
            ids[i, : len(s)] = torch.tensor(s)
            attention[i, : len(s)] = 1
            counted[i, max(e.start, 1) - 1 : len(s) - 1] = True
            i += 1
    ids, attention, counted = ids.to(device), attention.to(device), counted.to(device)
    logits = model(input_ids=ids, attention_mask=attention).logits[:, :-1].float()
    token_lp = -F.cross_entropy(logits.transpose(1, 2), ids[:, 1:], reduction="none")
    totals = (token_lp * counted).sum(dim=1)
    return list(torch.split(totals, [len(e.sequences) for e in batch]))


def losses(model, batch: list[Encoded], lm_weight: float):
    """（全体の損失, decision loss, 正解文の言語モデル損失）を返す。いずれも例の平均。"""
    log_probs = candidate_log_probs(model, batch)
    decision = torch.stack([decision_loss(lp, e.gold) for lp, e in zip(log_probs, batch)]).mean()
    # 正解の候補（と EOS）のトークンあたりの負の対数尤度
    lm = torch.stack(
        [-lp[e.gold] / (len(e.sequences[e.gold]) - e.start) for lp, e in zip(log_probs, batch)]
    ).mean()
    total = decision if lm_weight == 0 else decision + lm_weight * lm
    return total, decision, lm


def fit_temperature(log_probs: list[torch.Tensor], golds: list[int]) -> float:
    """候補集合の上の softmax(log P / T) の負の対数尤度が最小になる温度 T を求める。"""
    pairs = [(lp.detach().float().cpu(), g) for lp, g in zip(log_probs, golds) if len(lp) > 1]
    if not pairs:
        return 1.0
    log_t = torch.zeros((), requires_grad=True)
    optimizer = torch.optim.LBFGS([log_t], lr=0.1, max_iter=200, line_search_fn="strong_wolfe")

    def closure():
        optimizer.zero_grad()
        t = log_t.exp()
        loss = torch.stack([decision_loss(lp / t, g) for lp, g in pairs]).mean()
        loss.backward()
        return loss

    optimizer.step(closure)
    return float(log_t.detach().exp().clamp(0.05, 20.0))


def evaluate(model, examples: list[Encoded], batch_size: int):
    """dev の log P（例ごと）と、decision loss、top-1 を返す。"""
    model.eval()
    log_probs = []
    with torch.no_grad():
        for i in range(0, len(examples), batch_size):
            log_probs += [lp.cpu() for lp in candidate_log_probs(model, examples[i : i + batch_size])]
    if not examples:
        return log_probs, {"examples": 0}
    loss = sum(decision_loss(lp, e.gold).item() for lp, e in zip(log_probs, examples)) / len(examples)
    top1 = sum(int(lp.argmax()) == e.gold for lp, e in zip(log_probs, examples)) / len(examples)
    return log_probs, {"examples": len(examples), "decision_loss": loss, "top1": top1}


def llama_cpp_dir() -> Path:
    """scripts/fetch-zenz.sh が取ってくる、推論と同じ版の llama.cpp の変換スクリプトの場所。"""
    script = (ROOT / "scripts/fetch-zenz.sh").read_text()
    found = re.search(r'^LLAMA_CPP_COMMIT="([0-9a-f]+)"', script, re.M)
    if not found:
        raise ValueError("scripts/fetch-zenz.sh: LLAMA_CPP_COMMIT not found (pass --llama-cpp)")
    return ROOT / f"data/llama.cpp-{found.group(1)}"


def export_gguf(model_dir: Path, out: Path, outtype: str, llama_cpp: Path) -> None:
    if not (llama_cpp / "convert_hf_to_gguf.py").is_file():
        raise FileNotFoundError(f"{llama_cpp}: llama.cpp converter not found (run scripts/fetch-zenz.sh)")
    subprocess.run(
        [sys.executable, str(ROOT / "scripts/convert_zenz.py"), str(llama_cpp), str(model_dir), str(out), outtype],
        check=True,
        stdout=subprocess.DEVNULL,
    )


def read_examples(path: Path):
    """学習に使う例（aiokey.datagen の JSONL）を読む。"""
    with open(path, encoding="utf-8") as f:
        for no, line in enumerate(f, 1):
            if not line.strip():
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError as err:
                raise ValueError(f"{path}:{no}: {err}") from None
            if not isinstance(e, dict):
                raise ValueError(f"{path}:{no}: expected a JSON object")
            missing = [k for k in ("input", "target", "candidates") if k not in e]
            if missing:
                raise ValueError(f"{path}:{no}: missing {', '.join(missing)}")
            # 文脈はなくてもよい（null も空として扱う）
            e["context"] = e.get("context") or ""
            strings = [e["context"], e["input"], e["target"]]
            if not isinstance(e["candidates"], list) or not all(isinstance(x, str) for x in strings + e["candidates"]):
                raise ValueError(f"{path}:{no}: context, input, target and candidates must be strings")
            yield e


def _encode_all(tokenizer, path: Path, max_candidates: int, limit: int | None) -> list[Encoded]:
    out = []
    for e in read_examples(path):
        if limit is not None and len(out) >= limit:
            break
        enc = encode(tokenizer, e, max_candidates)
        if enc is not None:
            out.append(enc)
    return out


def run(args) -> dict:
    device = resolve_device(args.device)
    random.seed(args.seed)
    torch.manual_seed(args.seed)
    model, tokenizer = load(args.base, device)
    train_set = _encode_all(tokenizer, args.train, args.max_candidates, args.limit)
    dev_set = _encode_all(tokenizer, args.dev, args.max_candidates, args.limit)
    if not train_set:
        raise ValueError(f"{args.train}: no training examples")

    steps_per_epoch = math.ceil(len(train_set) / args.batch_size)
    total_steps = steps_per_epoch * args.epochs
    warmup = max(1, int(total_steps * args.warmup))
    optimizer = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=args.weight_decay)
    # 線形のウォームアップのあと、0 まで線形に下げる
    schedule = torch.optim.lr_scheduler.LambdaLR(
        optimizer, lambda s: min((s + 1) / warmup, max(0.0, (total_steps - s) / max(1, total_steps - warmup)))
    )
    # GPU では bf16 の自動混合精度で速くする（重みと最適化は f32 のまま）
    autocast = torch.autocast("cuda", dtype=torch.bfloat16) if device == "cuda" else contextlib.nullcontext()

    step, started, history = 0, time.monotonic(), []
    for epoch in range(args.epochs):
        model.train()
        order = list(range(len(train_set)))
        random.shuffle(order)
        for i in range(0, len(order), args.batch_size):
            batch = [train_set[j] for j in order[i : i + args.batch_size]]
            with autocast:
                total, decision, lm = losses(model, batch, args.lm_weight)
            optimizer.zero_grad(set_to_none=True)
            total.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()
            schedule.step()
            step += 1
            if step % args.log_every == 0 or step == total_steps:
                print(
                    f"epoch {epoch + 1} step {step}/{total_steps} loss {total.item():.4f} "
                    f"decision {decision.item():.4f} lm {lm.item():.4f} ({time.monotonic() - started:.0f}s)",
                    file=sys.stderr,
                )
        _, dev_report = evaluate(model, dev_set, args.batch_size)
        history.append({"epoch": epoch + 1, **dev_report})
        print(f"epoch {epoch + 1} dev {json.dumps(dev_report)}", file=sys.stderr)

    dev_log_probs, dev_report = evaluate(model, dev_set, args.batch_size)
    temperature = fit_temperature(dev_log_probs, [e.gold for e in dev_set])

    args.out.mkdir(parents=True, exist_ok=True)
    hf = args.out / "hf"
    # zenz の config.json は eos_token_id が 2（語彙では <s>）で、語彙の </s>（3）と食い違う。学習は </s> で
    # 終わる列で行うので、GGUF にも </s> が EOS として入るように、特殊トークンの ID を語彙に合わせて保存する
    for config in (model.config, model.generation_config):
        config.bos_token_id = tokenizer.bos_token_id
        config.eos_token_id = tokenizer.eos_token_id
        config.pad_token_id = tokenizer.pad_token_id
    model.save_pretrained(str(hf))
    for name in TOKENIZER_FILES:
        if (args.base / name).is_file():
            shutil.copy2(args.base / name, hf / name)
    export_gguf(hf, args.out / "aiokey-stage2-q8_0.gguf", "q8_0", args.llama_cpp)
    if args.f32:
        export_gguf(hf, args.out / "aiokey-stage2-f32.gguf", "f32", args.llama_cpp)

    report = {
        "device": device,
        "base": str(args.base),
        "train_examples": len(train_set),
        "epochs": args.epochs,
        "steps": step,
        "batch_size": args.batch_size,
        "lr": args.lr,
        "lm_weight": args.lm_weight,
        "max_candidates": args.max_candidates,
        "seconds": round(time.monotonic() - started, 1),
        "temperature": temperature,
        "dev": dev_report,
        "history": history,
    }
    (args.out / "train.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="aiokey train", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("--train", type=Path, required=True, help="aiokey.datagen が作った train.jsonl")
    p.add_argument("--dev", type=Path, required=True, help="温度スケーリングと検証に使う dev.jsonl")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--base", type=Path, default=ROOT / "data/zenz-v2.5-xsmall", help="元のモデル（Hugging Face 形式）")
    p.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    p.add_argument("--epochs", type=int, default=1)
    p.add_argument("--batch-size", type=int, default=8, help="1 ステップの例の数（候補はその全部）")
    p.add_argument("--lr", type=float, default=5e-5)
    p.add_argument("--weight-decay", type=float, default=0.01)
    p.add_argument("--warmup", type=float, default=0.05, help="ウォームアップの割合（全ステップに対して）")
    p.add_argument("--lm-weight", type=float, default=0.1, help="正解文の言語モデル損失の混合比。0 で decision loss だけ")
    p.add_argument("--max-candidates", type=int, default=32, help="1 例あたりの候補の上限（正解は必ず残す）")
    p.add_argument("--limit", type=int, help="train と dev のそれぞれで、使える例をこの数まで使う（長すぎて落とした例は数えない）")
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--log-every", type=int, default=50)
    p.add_argument("--f32", action="store_true", help="答え合わせ用に f32 の GGUF も書く")
    p.add_argument("--llama-cpp", type=Path, default=None, help="llama.cpp の変換スクリプトのフォルダ")
    return p


def main(argv: list[str] | None = None) -> int:
    p = parser()
    args = p.parse_args(argv)
    if args.batch_size < 1 or args.epochs < 1 or args.max_candidates < 1:
        p.error("--batch-size, --epochs and --max-candidates must be at least 1")
    if args.lm_weight < 0:
        p.error("--lm-weight must not be negative")
    try:
        args.llama_cpp = args.llama_cpp or llama_cpp_dir()
        report = run(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    print(json.dumps({k: report[k] for k in ("device", "steps", "temperature", "dev")}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
