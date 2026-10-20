"""変換の精度を測る。

    python -m aiokey eval --examples data/examples/test.jsonl --dict build/aiokey.dic -k 32

例（aiokey.datagen が作る JSONL）の入力を段1にかけ直し、次を JSON で出す。

- recall_at_k：正解が上位 K 件に入る割合（段2が当てられる上限）
- top1 / top5：正解が 1 位 / 上位 5 件に入る割合
- keystroke_reduction：打鍵削減率。1 −（AI-O Key の入力の打鍵数 ÷ 通常のローマ字入力の打鍵数）
- english_top1：英単語を含む文だけの top-1
- by_kind：水増しの種類（子音だけ・母音あり × 全文・途中まで）ごとの再現率と top-1
"""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Iterable, Iterator

# ローマ字で 1 打鍵の文字（ア行の母音）と、拗音・小書きの母音
VOWELS = set("あいうえお")
SMALL_YOON = set("ゃゅょ")
SMALL_VOWELS = set("ぁぃぅぇぉゎ")
REQUIRED = ("input", "target", "reading")


class ExamplesError(Exception):
    """評価データの形が正しくない。"""


def romaji_keys(reading: str) -> int:
    """読みを通常のローマ字入力（訓令式・ヘボン式の短い方）で打つときの打鍵数。

    - あいうえお は 1、ほかのかなは 2（子音 + 母音）
    - ん は、子音（な行・や行を除く）の前なら 1（n）、母音・な行・や行の前と語末では 2（nn）
    - っ は 1（次の子音を重ねる）、ー は 1
    - ゃゅょ は 1 足す（き 2 + ょ 1 = kyo の 3）。小書きの ぁぃぅぇぉゎ は 0（ふぁ = fa）
    - かな以外（英字・数字・記号）は 1 文字 1 打鍵
    """
    hira = [chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in reading]
    keys = 0
    for i, h in enumerate(hira):
        if h == "ん":
            nxt = hira[i + 1] if i + 1 < len(hira) else ""
            keys += 1 if _starts_with_plain_consonant(nxt) else 2
        elif h in VOWELS or h in ("っ", "ー") or h in SMALL_YOON:
            keys += 1
        elif h in SMALL_VOWELS:
            keys += 0
        elif "ぁ" <= h <= "ゖ":
            keys += 2
        else:
            keys += 1
    return keys


# ん を n 1 打鍵で打てない次のかな（母音・な行・や行。nn にしないと別のかなになる）
_NN_BEFORE = set("あいうえおなにぬねのやゆよ")


def _starts_with_plain_consonant(kana: str) -> bool:
    return "ぁ" <= kana <= "ゖ" and kana not in _NN_BEFORE and kana not in SMALL_VOWELS | SMALL_YOON


def read_examples(path: Path) -> Iterator[dict]:
    """評価データ（JSONL）を読む。壊れた行や、必要な欄のない行は、行番号つきでエラーにする。"""
    with open(path, encoding="utf-8") as f:
        for no, line in enumerate(f, 1):
            if not line.strip():
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError as err:
                raise ExamplesError(f"{path}:{no}: not JSON: {err}") from err
            if not isinstance(e, dict):
                raise ExamplesError(f"{path}:{no}: not a JSON object")
            tokens = e.get("tokens", [])
            if not isinstance(tokens, list) or not all(isinstance(t, dict) for t in tokens):
                raise ExamplesError(f"{path}:{no}: tokens must be a list of objects")
            missing = [key for key in REQUIRED if not isinstance(e.get(key), str)]
            if missing:
                raise ExamplesError(f"{path}:{no}: missing {', '.join(missing)}")
            yield e


def _has_english(e: dict) -> bool:
    return any(t.get("english") for t in e.get("tokens", []))


def _kind(e: dict) -> str:
    vowel = "vowels" if e.get("vowels_kept") else "consonants"
    cut = "prefix" if e.get("truncated") else "full"
    return f"{vowel}/{cut}"


def _rate(hits: int, total: int) -> float:
    return hits / total if total else 0.0


def summarize(examples: Iterable[dict], k: int) -> dict:
    """候補（`candidates`）の付いた例から、指標をまとめる。"""
    total = top1 = top5 = recall = 0
    typed = romaji = 0
    english = english_top1 = 0
    kinds: dict[str, list[int]] = {}
    for e in examples:
        candidates = e["candidates"][:k]
        target = e["target"]
        rank = candidates.index(target) + 1 if target in candidates else None
        total += 1
        recall += rank is not None
        top1 += rank == 1
        top5 += rank is not None and rank <= 5
        typed += len(e["input"])
        romaji += romaji_keys(e["reading"])
        if _has_english(e):
            english += 1
            english_top1 += rank == 1
        stats = kinds.setdefault(_kind(e), [0, 0, 0])
        stats[0] += 1
        stats[1] += rank is not None
        stats[2] += rank == 1
    return {
        "examples": total,
        "k": k,
        "recall_at_k": _rate(recall, total),
        "top1": _rate(top1, total),
        "top5": _rate(top5, total),
        "keystroke_reduction": 1 - typed / romaji if romaji else 0.0,
        "english_examples": english,
        "english_top1": _rate(english_top1, english),
        "by_kind": {
            kind: {"examples": n, "recall_at_k": _rate(r, n), "top1": _rate(t, n)}
            for kind, (n, r, t) in sorted(kinds.items())
        },
    }


def evaluate(examples: Iterable[dict], converter, k: int, scorer=None) -> dict:
    """例の入力を段1にかけ直して、指標をまとめる。`converter` は `convert_with_readings(input, k)` を持つもの
    （`aiokey.Converter`）。`scorer`（`aiokey.Scorer`）を渡すと、段1の候補を段2の確率で並べ替えてから数え、
    採点にかかった時間の p50・p95（ミリ秒）も出す。"""
    times: list[float] = []

    def candidates(e: dict) -> list[str]:
        pairs = converter.convert_with_readings(e["input"], k)
        if scorer is None or not pairs:
            return [s for s, _ in pairs]
        started = time.perf_counter()
        probs, _ = scorer.score(e.get("context", ""), e["input"], pairs)
        times.append((time.perf_counter() - started) * 1000)
        ranked = sorted(range(len(probs)), key=lambda i: -probs[i])
        return [pairs[i][0] for i in ranked] + [s for s, _ in pairs[len(probs):]]

    report = summarize(({**e, "candidates": candidates(e)} for e in examples), k)
    if times:
        times.sort()
        at = lambda q: times[round((len(times) - 1) * q)]
        report["stage2_ms"] = {"p50": at(0.5), "p95": at(0.95), "max": times[-1], "calls": len(times)}
    return report
