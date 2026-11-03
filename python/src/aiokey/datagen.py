"""学習・評価データを作る。

1 文から 1 例を作る。入力は読みのスケルトンで、設定した割合で次の水増しをする。

- 母音を残す：かなのいくつかを、子音に母音を付けて打った形にする（混在入力向け）
- 途中で切る：文をトークンの切れ目で途中までにする（ライブ変換向け）
- 文脈あり：同じ記事の 1 つ前の文を文脈として付ける

各例には段1の候補集合（最大 K 件）を付ける。正解が候補にないときは正解を足し、漏れとして別に記録する。
評価に使う文（`--exclude` の JSONL の正解）と同じ文は、例にも文脈にも使わない。

    python -m aiokey.datagen --articles data/wiki/articles.jsonl data/corpora/articles/tatoeba.jsonl \
        --exclude target/eval/atc.jsonl --dict build/aiokey.dic --out data/examples --workers 8

test を作らないとき（`--splits train,dev`）は、test の記事に出てくる文を train / dev でも使わない（例を作ったあとで除く）。
例は作りながら書き出す（数百万例でもメモリに持たない）。train と dev には学習に使わない項目（`tokens`、`skeleton`、
`reading`）を書かず、`missing.jsonl` には漏れの記録に要る項目だけを書く。`--train-in-stage1-only` を付けると、train には
段1の候補に正解がある例だけを書く（学習は既定でその例だけを使うので、結果は同じでファイルが 3 割ほどになる）。乱数は記事ごとに（入力の中の位置と記事 ID から）決めるので、
`--workers` の数によらず同じ例ができる（位置は `--splits` で絞ったあとの並びで数えるので、`--splits` を変えると変わる）。`stats.json` に、コーパスごと（記事 ID の `:` の前。ないものは wikipedia）の
記事・例の数と、段1の候補に正解がある例の数を書く。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import sys
import unicodedata
from collections import Counter, deque
from dataclasses import dataclass
from pathlib import Path
from multiprocessing import Pool
from typing import Iterable, Iterator

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


def normalize(sentence: str) -> str:
    """評価の文と比べるときの形（全角・半角の違いと前後の空白を無視する）。"""
    return unicodedata.normalize("NFKC", sentence).strip()


def eval_sentences(paths: Iterable[Path]) -> set[str]:
    """評価の例（JSONL）の正解の文。学習データから除くのに使う。"""
    out = set()
    for path in paths:
        with open(path, encoding="utf-8") as f:
            for number, line in enumerate(f, 1):
                if not line.strip():
                    continue
                try:
                    out.add(normalize(json.loads(line)["target"]))
                except (json.JSONDecodeError, KeyError, TypeError) as e:
                    raise ValueError(f"{path}: line {number}: expected a JSON object with 'target' ({e!r})") from None
    return out


def generate(
    articles: Iterable[tuple[str, list[str]]],
    converter,
    config: Config,
    k: int,
    annotator: rd.Annotator | None = None,
    exclude: set[str] | frozenset[str] = frozenset(),
    start: int = 0,
    drop: set[str] | frozenset[str] = frozenset(),
) -> tuple[list[dict], list[dict]]:
    """記事（ID と文の並び）から例を作る。`converter` が None なら候補集合は正解だけにする。

    `exclude`（[`normalize`] した文）に入っている文は、例にも次の文の文脈にも使わない。`start` は最初の記事の
    入力の中での位置（乱数を記事ごとに決めるのに使う）。`drop` に入っている文も、例にも文脈にも使わないが、
    例を作ったあと（乱数を使ったあと）で除く。ほかの例の作り方を変えずに、あとから除く文を足せる。
    """
    annotator = annotator or rd.Annotator()
    examples, missing = [], []
    cache: dict[str, list[rd.Token]] = {}
    for position, (article, sentences) in enumerate(articles, start):
        rng = random.Random(f"{config.seed}:{position}:{article}")
        previous = ""
        for index, sentence in enumerate(sentences):
            if exclude and normalize(sentence) in exclude:
                previous = ""
                continue
            if sentence not in cache:
                cache[sentence] = annotator.tokens(sentence)
            tokens = cache[sentence]
            example = _make_example(article, index, tokens, previous, rng, config)
            previous = sentence
            if example is None:
                continue
            if drop and normalize(sentence) in drop:
                continue
            if drop and example["context"] and normalize(example["context"]) in drop:
                example["context"], example["has_context"] = "", False
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


def english_counts(examples: Iterable[dict]) -> Counter:
    """例に出てくる英単語（英字だけのトークン）の数。"""
    return Counter(t["surface"] for e in examples for t in e.get("tokens", []) if t.get("english"))


def write_english_tsv(examples: Iterable[dict], path: Path, counts: Counter | None = None) -> None:
    """例に出てくる英単語を数え（`counts` を渡せばそれを使い）、辞書ビルダーが読める頻度 TSV に書く。"""
    counts = english_counts(examples) if counts is None else counts
    with open(path, "w", encoding="utf-8") as f:
        f.write("# コーパスに出てきた英単語の頻度（aiokey.datagen が作る）\n")
        for word, n in sorted(counts.items(), key=lambda x: (-x[1], x[0])):
            f.write(f"{word}\t{n}\n")


def _read_articles(path: Path) -> Iterable[tuple[str, list[str]]]:
    with open(path, encoding="utf-8") as f:
        for line in f:
            a = json.loads(line)
            yield a["id"], a["sentences"]


def test_sentences(paths: Iterable[Path]) -> frozenset[str]:
    """test に分けた記事の文（[`normalize`] した形）。train / dev では使わない。

    評価の例は正解を途中で切ったものがあり（`truncate_rate`）、その元の文全体は評価の例に残らない。test の記事の文を
    まるごと除いておけば、ほかの記事やコーパスに同じ文があっても学習に入らない（#41 のレビュー）。
    """
    return frozenset(normalize(s) for path in paths for a, sentences in _read_articles(path)
                     if split_of(a) == "test" for s in sentences)


def source_of(article: str) -> str:
    """記事 ID からコーパスの名前（aiokey.sources は `名前:` を前に付ける。Wikipedia は付けない）。"""
    return article.split(":", 1)[0] if ":" in article else "wikipedia"


# train と dev に書かない項目（学習に使わない。評価の例を作る test には残す）
UNUSED_IN_TRAINING = ("tokens", "skeleton", "reading")

# 1 つの仕事（ワーカーに渡すまとまり）の文の数の上限
CHUNK_SENTENCES = 2000

# 並列に作るときの、ワーカーごとの変換器と読みの解析器
_worker: dict = {}


def _init_worker(dict_path: str, config: Config, k: int, exclude: frozenset[str], train_in_stage1_only: bool,
                 drop: frozenset[str]) -> None:
    from aiokey import Converter

    _worker.update(converter=Converter(dict_path), annotator=rd.Annotator(), config=config, k=k, exclude=exclude,
                   train_in_stage1_only=train_in_stage1_only, drop=drop)


def _generate_chunk(chunk: tuple[int, list[tuple[str, list[str]]]]):
    """記事のまとまりから例を作り、書き出す行にして返す（ワーカーの中で JSON にして、受け渡しを小さくする）。

    返すのは（[(分割, 書く行 か None, 記事 ID, 段1の候補に正解があるか)], 漏れの行, 英単語の数）。
    """
    start, articles = chunk
    w = _worker
    examples, missing = generate(articles, w["converter"], w["config"], w["k"], w["annotator"], w["exclude"], start,
                                 w["drop"])
    rows = []
    for e in examples:
        split = split_of(e["article"])
        line = None
        if split != "train" or e["gold_in_stage1"] or not w["train_in_stage1_only"]:
            row = e if split == "test" else {k: v for k, v in e.items() if k not in UNUSED_IN_TRAINING}
            line = json.dumps(row, ensure_ascii=False) + "\n"
        rows.append((split, line, e["article"], e["gold_in_stage1"]))
    missed = [
        json.dumps({k: e[k] for k in ("article", "sentence_index", "context", "input", "target")}, ensure_ascii=False)
        + "\n"
        for e in missing
    ]
    return rows, missed, english_counts(examples)


def _bounded_imap(pool, items: Iterable, window: int) -> Iterator:
    """順番を保って並列に流し、まだ受け取っていない仕事を `window` 個までに抑える
    （`Pool.imap` は入力を先にすべて読み込むので、数百万文のコーパスがメモリに載ってしまう）。"""
    pending: deque = deque()
    for item in items:
        pending.append(pool.apply_async(_generate_chunk, (item,)))
        if len(pending) >= window:
            yield pending.popleft().get()
    while pending:
        yield pending.popleft().get()


def _chunks(articles: Iterable[tuple[str, list[str]]], max_sentences: int) -> Iterator[tuple[int, list]]:
    """記事を、文の数が `max_sentences` までのまとまりに分ける（1 記事だけで超えるなら、その記事で 1 まとまり）。
    Wikipedia の長い記事は 1 つで数百文あるので、記事の数で区切るとまとまりの大きさが大きくばらつく。"""
    chunk, start, sentences = [], 0, 0
    for position, article in enumerate(articles):
        if chunk and sentences + len(article[1]) > max_sentences:
            yield start, chunk
            chunk, sentences = [], 0
        if not chunk:
            start = position
        chunk.append(article)
        sentences += len(article[1])
    if chunk:
        yield start, chunk


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--articles", type=Path, nargs="+", default=[Path("data/wiki/articles.jsonl")],
                        help="記事の JSONL（aiokey.corpus か aiokey.sources が作ったもの）。いくつでも")
    parser.add_argument("--exclude", type=Path, nargs="*", default=[],
                        help="評価の例の JSONL。正解と同じ文は学習データに使わない")
    parser.add_argument("--workers", type=int, default=1, help="並列に例を作るプロセスの数")
    parser.add_argument("--train-in-stage1-only", action="store_true",
                        help="train には段1の候補に正解がある例だけを書く（学習は既定でその例だけを使う）")
    parser.add_argument("--dict", type=Path, default=Path("build/aiokey.dic"))
    parser.add_argument("--out", type=Path, default=Path("data/examples"))
    parser.add_argument("-k", type=int, default=32)
    parser.add_argument("--vowel-rate", type=float, default=Config.vowel_rate)
    parser.add_argument("--truncate-rate", type=float, default=Config.truncate_rate)
    parser.add_argument("--context-rate", type=float, default=Config.context_rate)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument(
        "--splits",
        default="train,dev,test",
        help="作る分割（カンマ区切り）。評価だけなら test にすると、ほかの記事を読み飛ばして速い",
    )
    args = parser.parse_args(argv)
    wanted = {s.strip() for s in args.splits.split(",") if s.strip()}
    if not wanted or not wanted <= {"train", "dev", "test"}:
        parser.error("--splits takes names from train, dev, test (comma-separated)")
    if args.k < 1:
        parser.error("-k must be at least 1")

    if args.workers < 1:
        parser.error("--workers must be at least 1")

    config = Config(args.vowel_rate, args.truncate_rate, args.context_rate, args.seed)
    exclude = frozenset(eval_sentences(args.exclude))
    # test を作らないときは、test の記事の文を train / dev から除く（作るときは test の例に要るので除かない）
    drop = test_sentences(args.articles) if "test" not in wanted else frozenset()
    articles = (a for path in args.articles for a in _read_articles(path) if split_of(a[0]) in wanted)

    # 拡張モジュール（辞書）は、引数を確かめてから読み込む。並列のときも先にここで読んで確かめる
    # （ワーカーの初期化で失敗すると、プールがワーカーを作り直し続けて終わらない）
    from aiokey import Converter

    Converter(str(args.dict))
    # 途中で失敗したときに、途中までのファイルが出力として残らないよう、書き終えてから名前を変える
    args.out.mkdir(parents=True, exist_ok=True)
    names = [*sorted(wanted), "missing"]
    files = {name: open(args.out / f"{name}.jsonl.partial", "w", encoding="utf-8") for name in names}
    english: Counter = Counter()
    stats: dict[str, Counter] = {}
    seen_articles: set[str] = set()
    pool = None
    try:
        init = (str(args.dict), config, args.k, exclude, args.train_in_stage1_only, drop)
        chunks = _chunks(articles, CHUNK_SENTENCES)
        if args.workers == 1:
            _init_worker(*init)
            results = map(_generate_chunk, chunks)
        else:
            pool = Pool(args.workers, _init_worker, init)
            results = _bounded_imap(pool, chunks, 3 * args.workers)
        for rows, missed, counts in results:
            for split, line, article, gold in rows:
                if line is not None:
                    files[split].write(line)
                s = stats.setdefault(source_of(article), Counter())
                s["examples"] += 1
                s["gold_in_stage1"] += gold
                if article not in seen_articles:
                    seen_articles.add(article)
                    s["articles"] += 1
            files["missing"].writelines(missed)
            english.update(counts)
        if pool is not None:
            pool.close()
            pool.join()
            pool = None
    finally:
        if pool is not None:
            pool.terminate()
        for f in files.values():
            f.close()
    for name in names:
        (args.out / f"{name}.jsonl.partial").replace(args.out / f"{name}.jsonl")
    write_english_tsv([], args.out / "english.tsv", english)
    (args.out / "stats.json").write_text(
        json.dumps({name: dict(s) for name, s in sorted(stats.items())}, indent=2) + "\n", encoding="utf-8"
    )
    total = sum(s["examples"] for s in stats.values())
    if total:
        hit = sum(s["gold_in_stage1"] for s in stats.values()) / total
        print(f"{total} examples, gold in stage1 top-{args.k}: {hit:.1%}", file=sys.stderr)
    else:
        print("no examples (no sentences in the articles)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
