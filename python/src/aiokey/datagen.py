"""学習・評価データを作る。"""

from dataclasses import dataclass


@dataclass
class Config:
    vowel_rate: float = 0.0
    truncate_rate: float = 0.0
    context_rate: float = 0.0
    seed: int = 0


def generate(articles, converter, config, k):
    raise NotImplementedError


def split_of(article):
    raise NotImplementedError


def split_examples(examples):
    raise NotImplementedError


def write_english_tsv(examples, path):
    raise NotImplementedError
