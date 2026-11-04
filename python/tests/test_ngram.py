"""Issue #58 基準 1：文字 n-gram の表を作るコマンド。"""

import json

import pytest

import aiokey
from aiokey import datagen
from aiokey.__main__ import main


def article_id(prefix, split):
    """split_of がその分割に分ける記事 ID。"""
    return next(i for i in (f"{prefix}{n}" for n in range(10000)) if datagen.split_of(i) == split)


def write_articles(path, articles):
    path.write_text(
        "".join(json.dumps({"id": i, "title": i, "sentences": s}, ensure_ascii=False) + "\n" for i, s in articles),
        encoding="utf-8",
    )
    return path


def test_table_is_built_only_from_train_articles_of_the_allowed_corpora(tmp_path):
    # train の記事の文だけを数える。Web のコーパス、dev と test の記事、評価の文は入れない
    used = "果たして本当に良い。"
    never = {
        "web": "雲丹鮨旨過玄人衆。",
        "dev": "鯨骨群集駿河湾底。",
        "test": "蜂球熱殺雀蜂退治。",
        "eval": "飴玉甘味駄菓子屋。",
    }
    articles = write_articles(tmp_path / "articles.jsonl", [
        (article_id("", "train"), [used, never["eval"]]),
        (article_id("tatoeba:", "train"), [used]),
        (article_id("web:", "train"), [never["web"]]),
        (article_id("", "dev"), [never["dev"]]),
        (article_id("", "test"), [never["test"]]),
    ])
    exclude = tmp_path / "eval.jsonl"
    exclude.write_text(json.dumps({"target": never["eval"]}, ensure_ascii=False) + "\n", encoding="utf-8")
    out = tmp_path / "aiokey-ngram.bin"

    assert main(["ngram", "--articles", str(articles), "--exclude", str(exclude), "--out", str(out),
                 "--min-count", "1"]) == 0

    table = aiokey.Ngram(str(out))
    seen = table.cost(used)
    for name, sentence in never.items():
        # 同じ長さの知らない文は、知っている文よりずっとコストが高い
        assert table.cost(sentence) > 3 * seen, name


def test_malformed_table_is_rejected(tmp_path):
    # 信頼の境界：表のファイルが壊れていたら、開く時点で理由つきで失敗する
    path = tmp_path / "broken.bin"
    path.write_bytes(b"AIOKNGR1" + b"\0" * 40)
    with pytest.raises(ValueError, match="ngram"):
        aiokey.Ngram(str(path))
