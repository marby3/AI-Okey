"""段1の候補の並べ替えに使う文字 n-gram の表を作る（#58）。

    python -m aiokey ngram --articles data/corpora/articles/wikipedia.jsonl ... \
        --exclude target/eval/atc.jsonl target/eval/test.jsonl target/eval/proper.jsonl --out target/ngram/aiokey-ngram.bin

数えるのは許可したコーパスの train の記事の文だけ。Web のコーパス、dev と test の記事、評価の文は入れない
（docs/training-data.md）。表が持つのは 3 文字までの並びと確率だけで、文章そのものは入らない。
コストの計算は stupid backoff：3 文字 → 2 文字（×0.4）→ 1 文字（×0.16）。
"""

from __future__ import annotations

import math
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

from aiokey import datagen, write_ngram

# 表を作るのに使ってよいコーパス（記事 ID の `:` の前。Wikipedia は付かない）
ALLOWED_SOURCES = frozenset({"wikipedia", "wikinews", "tatoeba", "commonvoice", "aozora"})
BOS = "\0"
BACKOFF_2 = 0.4
BACKOFF_1 = 0.16
MAX_COST = 65535
# 2・3 文字の並びの足切り（dev で選んだ。docs/eval/stage1-ngram.md）
MIN_COUNT = 20


@dataclass
class Counts:
    trigrams: Counter = field(default_factory=Counter)
    bigrams: Counter = field(default_factory=Counter)
    unigrams: Counter = field(default_factory=Counter)
    # 文脈として出た回数（3 文字と 2 文字の確率の分母）
    context2: Counter = field(default_factory=Counter)
    context1: Counter = field(default_factory=Counter)
    sentences: int = 0

    def add(self, sentence: str) -> None:
        s = BOS + BOS + sentence
        for i in range(2, len(s)):
            self.trigrams[s[i - 2 : i + 1]] += 1
            self.bigrams[s[i - 1 : i + 1]] += 1
            self.unigrams[s[i]] += 1
            self.context2[s[i - 2 : i]] += 1
            self.context1[s[i - 1]] += 1
        self.sentences += 1


def count(articles: Iterable[Path], exclude: Iterable[Path] = ()) -> Counts:
    """train の記事の文を数える。評価の文と、test の記事に出てくる文は、ほかの記事にあっても数えない。"""
    paths = list(articles)
    leave_out = datagen.eval_sentences(exclude) | datagen.test_sentences(paths)
    counts = Counts()
    for path in paths:
        for article, sentences in datagen._read_articles(path):
            if datagen.source_of(article) not in ALLOWED_SOURCES or datagen.split_of(article) != "train":
                continue
            for sentence in sentences:
                if datagen.normalize(sentence) not in leave_out:
                    counts.add(sentence)
    return counts


def _cost(probability: float) -> int:
    return min(MAX_COST, round(-100 * math.log(probability)))


def entries(counts: Counts, min_count: int = MIN_COUNT) -> tuple[list[tuple[str, int]], int]:
    """表に書く (並び, コスト) の一覧と、知らない文字のコスト。`min_count` 回より少ない 2・3 文字の並びは持たない。"""
    total = sum(counts.unigrams.values())
    out = [(c, _cost(BACKOFF_1 * (n + 0.5) / total)) for c, n in counts.unigrams.items()]
    out += [(g, _cost(BACKOFF_2 * n / counts.context1[g[0]])) for g, n in counts.bigrams.items() if n >= min_count]
    out += [(g, _cost(n / counts.context2[g[:2]])) for g, n in counts.trigrams.items() if n >= min_count]
    return out, _cost(BACKOFF_1 * 0.5 / total) if total else MAX_COST


def write(counts: Counts, path: Path, min_count: int = MIN_COUNT) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    table, unknown = entries(counts, min_count)
    write_ngram(str(path), table, unknown)
