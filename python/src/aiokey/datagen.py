"""学習・評価データを作る。

1 文から 1 例を作る。入力は読みのスケルトンで、設定した割合で次の水増しをする。

- 母音を残す：かなのいくつかを、子音に母音を付けて打った形にする（混在入力向け）
- 途中で切る：文をトークンの切れ目で途中までにする（ライブ変換向け）
- 文脈あり：同じ記事の 1 つ前の文を文脈として付ける

各例には段1の候補集合（最大 K 件）を付ける。正解が候補にないときは正解を足し、漏れとして別に記録する。

    python -m aiokey.datagen --articles data/wiki/articles.jsonl --dict build/aiokey.dic --out data/examples
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from aiokey import reading as rd

# 記事の分割の割合（記事 ID のハッシュで決める）
DEV_SHARE = 0.05
TEST_SHARE = 0.05
# 母音を残す例で、かな 1 つあたり母音を残す確率
VOWEL_PER_KANA = 0.3


@dataclass
class Config:
    vowel_rate: float = 0.2
    truncate_rate: float = 0.2
    context_rate: float = 0.5
    seed: int = 0


def split_of(article: str) -> str:
    """記事 ID から分割（train / dev / test）を決める。同じ記事はいつも同じ分割になる。"""
    h = int.from_bytes(hashlib.sha256(article.encode()).digest()[:8], "big") / 2**64
    if h < TEST_SHARE:
        return "test"
    if h < TEST_SHARE + DEV_SHARE:
        return "dev"
    return "train"


def split_examples(examples: Iterable[dict]) -> dict[str, list[dict]]:
    parts: dict[str, list[dict]] = {"train": [], "dev": [], "test": []}
    for e in examples:
        parts[split_of(e["article"])].append(e)
    return parts


def generate(
    articles: Iterable[tuple[str, list[str]]],
    converter,
    config: Config,
    k: int,
    annotator: rd.Annotator | None = None,
) -> tuple[list[dict], list[dict]]:
    """記事（ID と文の並び）から例を作る。`converter` が None なら候補集合は正解だけにする。"""
    annotator = annotator or rd.Annotator()
    rng = random.Random(config.seed)
    examples, missing = [], []
    cache: dict[str, list[rd.Token]] = {}
    for article, sentences in articles:
        previous = ""
        for index, sentence in enumerate(sentences):
            if sentence not in cache:
                cache[sentence] = annotator.tokens(sentence)
            tokens = cache[sentence]
            example = _make_example(article, index, tokens, previous, rng, config)
            previous = sentence
            if example is None:
                continue
            candidates = converter.convert(example["input"], k) if converter else []
            example["gold_in_stage1"] = example["target"] in candidates
            if not example["gold_in_stage1"]:
                candidates = candidates[: max(k - 1, 0)] + [example["target"]]
                missing.append(example)
            example["candidates"] = candidates
            examples.append(example)
    return examples, missing


def _make_example(article, index, tokens, previous, rng, config) -> dict | None:
    truncated = len(tokens) > 1 and rng.random() < config.truncate_rate
    if truncated:
        tokens = tokens[: rng.randint(1, len(tokens) - 1)]
    total_units = sum(rd.unit_count(t) for t in tokens)
    vowels_kept = total_units > 0 and rng.random() < config.vowel_rate
    keep: set[int] = set()
    if vowels_kept:
        keep = {i for i in range(total_units) if rng.random() < VOWEL_PER_KANA}
        if not keep:
            keep = {rng.randrange(total_units)}
    has_context = bool(previous) and rng.random() < config.context_rate
    target = "".join(t.surface for t in tokens)
    if not target:
        return None
    return {
        "article": article,
        "sentence_index": index,
        "context": previous if has_context else "",
        "input": rd.to_input(tokens, keep),
        "skeleton": rd.to_input(tokens),
        "target": target,
        "reading": "".join(t.reading for t in tokens),
        "tokens": [t.to_dict() for t in tokens],
        "vowels_kept": vowels_kept,
        "truncated": truncated,
        "has_context": has_context,
    }


def write_english_tsv(examples: Iterable[dict], path: Path) -> None:
    """例に出てくる英単語（英字だけのトークン）を数え、辞書ビルダーが読める頻度 TSV に書く。"""
    counts = Counter(
        t["surface"] for e in examples for t in e.get("tokens", []) if t.get("english")
    )
    with open(path, "w", encoding="utf-8") as f:
        f.write("# コーパスに出てきた英単語の頻度（aiokey.datagen が作る）\n")
        for word, n in sorted(counts.items(), key=lambda x: (-x[1], x[0])):
            f.write(f"{word}\t{n}\n")


def _read_articles(path: Path) -> Iterable[tuple[str, list[str]]]:
    with open(path, encoding="utf-8") as f:
        for line in f:
            a = json.loads(line)
            yield a["id"], a["sentences"]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--articles", type=Path, default=Path("data/wiki/articles.jsonl"))
    parser.add_argument("--dict", type=Path, default=Path("build/aiokey.dic"))
    parser.add_argument("--out", type=Path, default=Path("data/examples"))
    parser.add_argument("-k", type=int, default=32)
    parser.add_argument("--vowel-rate", type=float, default=Config.vowel_rate)
    parser.add_argument("--truncate-rate", type=float, default=Config.truncate_rate)
    parser.add_argument("--context-rate", type=float, default=Config.context_rate)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args(argv)
    if args.k < 1:
        parser.error("-k must be at least 1")

    # 拡張モジュール（辞書）は、引数を確かめてから読み込む
    from aiokey import Converter

    config = Config(args.vowel_rate, args.truncate_rate, args.context_rate, args.seed)
    examples, missing = generate(_read_articles(args.articles), Converter(str(args.dict)), config, args.k)
    args.out.mkdir(parents=True, exist_ok=True)
    for name, part in split_examples(examples).items():
        with open(args.out / f"{name}.jsonl", "w", encoding="utf-8") as f:
            for e in part:
                f.write(json.dumps(e, ensure_ascii=False) + "\n")
    with open(args.out / "missing.jsonl", "w", encoding="utf-8") as f:
        for e in missing:
            f.write(json.dumps(e, ensure_ascii=False) + "\n")
    write_english_tsv(examples, args.out / "english.tsv")
    if examples:
        hit = 1 - len(missing) / len(examples)
        print(f"{len(examples)} examples, gold in stage1 top-{args.k}: {hit:.1%}", file=sys.stderr)
    else:
        print("no examples (no sentences in the articles)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
