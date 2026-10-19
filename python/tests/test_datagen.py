"""Issue #6 の受け入れテスト：コーパス処理と学習データ生成。"""

import bz2
import json
import os
from pathlib import Path

import pytest

import aiokey
from aiokey import corpus, datagen, reading

ROOT = Path(__file__).resolve().parents[2]
MOZC = Path(os.environ.get("AIOKEY_MOZC_DIR", ROOT / "data/mozc"))


def wiki_xml(pages):
    body = "".join(
        f"<page><title>{t}</title><ns>{ns}</ns><id>{i}</id>"
        + ("<redirect title='x'/>" if redirect else "")
        + f"<revision><text>{text}</text></revision></page>"
        for i, t, ns, redirect, text in pages
    )
    return f"<mediawiki xmlns='http://www.mediawiki.org/xml/export-0.11/'>{body}</mediawiki>".encode()


@pytest.fixture(scope="module")
def converter(tmp_path_factory):
    path = tmp_path_factory.mktemp("dict") / "aiokey.dic"
    aiokey.build_dictionary(str(MOZC), str(path))
    return aiokey.Converter(str(path))


def test_fetch_extracts_articles_from_a_pinned_dump_and_splits_sentences():
    assert len(corpus.DUMP_DATE) == 8 and corpus.DUMP_DATE.isdigit()
    assert corpus.DUMP_DATE in corpus.dump_url()
    xml = wiki_xml(
        [
            (1, "果物", 0, False, "'''りんご'''は[[果物]]である。とても赤い！{{参照}}"),
            (2, "転送", 0, True, "#REDIRECT [[果物]]"),
            (3, "ノート:果物", 1, False, "議論のページ。"),
            (4, "野菜", 0, False, "にんじんは野菜だ。"),
        ]
    )
    articles = list(corpus.extract_articles([bz2.compress(xml)], limit=1))
    assert len(articles) == 1
    assert articles[0].id == "1"
    assert articles[0].sentences == ["りんごは果物である。", "とても赤い！"]


def test_reading_skeleton_keeps_english_spelling():
    annotator = reading.Annotator()
    tokens = annotator.tokens("keyboardを買う")
    assert reading.to_input(tokens) == "keyboardwku"
    assert "".join(t.surface for t in tokens) == "keyboardを買う"


def test_augmentation_rates_follow_the_config():
    config = datagen.Config(vowel_rate=0.3, truncate_rate=0.2, context_rate=0.6, seed=1)
    sentences = [("a1", ["果たして本当にそうか。", "明日は晴れるでしょう。"])] * 600
    examples, _ = datagen.generate(sentences, converter=None, config=config, k=8)
    n = len(examples)
    assert n >= 1000
    rate = lambda flag: sum(e[flag] for e in examples) / n
    assert abs(rate("vowels_kept") - 0.3) < 0.05
    assert abs(rate("truncated") - 0.2) < 0.05
    # 文脈は前の文があるとき（記事の 2 文目以降）だけ付けられるので、その中での割合を見る
    with_previous = [e for e in examples if e["sentence_index"] > 0]
    context_rate = sum(e["has_context"] for e in with_previous) / len(with_previous)
    assert abs(context_rate - 0.6) < 0.05
    assert not any(e["has_context"] for e in examples if e["sentence_index"] == 0)
    # 母音を残した例は、子音だけの入力より長い
    assert all(len(e["input"]) >= len(e["skeleton"]) for e in examples)


def test_candidates_include_the_target_and_misses_are_recorded(converter):
    sentences = [("a1", ["果たして本当に"]), ("a2", ["ぬぽぽぺぴ"])]
    config = datagen.Config(vowel_rate=0, truncate_rate=0, context_rate=0, seed=1)
    examples, missing = datagen.generate(sentences, converter=converter, config=config, k=32)
    by_target = {e["target"]: e for e in examples}
    hit = by_target["果たして本当に"]
    assert hit["target"] in hit["candidates"] and len(hit["candidates"]) <= 32
    assert hit["gold_in_stage1"] is True
    miss = by_target["ぬぽぽぺぴ"]
    assert miss["target"] in miss["candidates"] and miss["gold_in_stage1"] is False
    assert [m["target"] for m in missing] == ["ぬぽぽぺぴ"]


def test_split_is_by_article():
    ids = [f"article-{i}" for i in range(2000)]
    split = {a: datagen.split_of(a) for a in ids}
    assert set(split.values()) == {"train", "dev", "test"}
    # 同じ記事はいつも同じ分割に入る
    assert all(datagen.split_of(a) == split[a] for a in ids)
    examples = [{"article": a} for a in ids for _ in range(3)]
    parts = datagen.split_examples(examples)
    seen = {}
    for name, part in parts.items():
        for e in part:
            assert seen.setdefault(e["article"], name) == name


def test_english_frequency_tsv_is_readable_by_the_dictionary_builder(tmp_path):
    examples = [
        {"tokens": [{"surface": "keyboard", "english": True}, {"surface": "を", "english": False}]},
        {"tokens": [{"surface": "keyboard", "english": True}, {"surface": "Linux", "english": True}]},
    ]
    path = tmp_path / "english.tsv"
    datagen.write_english_tsv(examples, path)
    words = dict(aiokey.read_english_tsv(str(path)))
    assert words == {"keyboard": 2, "Linux": 1}


def test_broken_dump_is_an_error_not_silence():
    # ダンプは外から取ってくるデータ。壊れていれば例外にし、0 件として黙って進まない
    with pytest.raises(corpus.CorpusError):
        list(corpus.extract_articles([b"BZh91AY&SY not really bzip2"], limit=10))
    truncated = bz2.compress(wiki_xml([(1, "a", 0, False, "文。")]))[:-20]
    with pytest.raises(corpus.CorpusError):
        list(corpus.extract_articles([truncated], limit=10))
