"""Wikipedia 以外のコーパスを、aiokey.corpus と同じ記事の形（ID、題、文の並び）にする。

取得は scripts/fetch-corpora.sh（版とハッシュを固定）。どのコーパスを使うか、ライセンス、Web のコーパスを使う
法的な根拠は docs/training-data.md に記録する。取得したデータも、ここで作る記事も、リポジトリと配布物には入れない
（data/ と target/ は .gitignore で除いている）。

    python -m aiokey.sources tatoeba --in data/corpora/tatoeba/jpn_sentences.tsv.bz2 --out data/corpora/articles/tatoeba.jsonl
    python -m aiokey.sources web --in data/corpora/fineweb-2/000_00000.parquet --limit 100000 --out ...

記事の ID にはコーパスの名前を前に付ける（`tatoeba:123`）。学習・評価の分割は記事の ID で決まるので
（aiokey.datagen）、ID がコーパスどうしで重ならないようにするため。
"""

from __future__ import annotations

import argparse
import bz2
import gzip
import json
import sys
from pathlib import Path
from typing import IO, Iterator

from aiokey.corpus import Article, split_sentences


# Web の文書は、メニュー・見出し・リンクの行が混じるので、文の終わりの記号で終わる行だけを文として使う
SENTENCE_FINAL = ("。", "！", "？", "!", "?", "」")


class SourceError(Exception):
    """コーパスのファイルを読めなかった（形が崩れている）。"""


def _open_text(path: Path) -> IO[bytes]:
    if path.suffix == ".bz2":
        return bz2.open(path, "rb")
    if path.suffix == ".gz":
        return gzip.open(path, "rb")
    return open(path, "rb")


def _lines(path: Path) -> Iterator[tuple[int, str]]:
    """行番号（1 始まり）と、改行を除いた行。UTF-8 でなければ行番号つきでエラーにする。"""
    with _open_text(path) as f:
        for number, raw in enumerate(f, 1):
            try:
                yield number, raw.decode("utf-8").rstrip("\r\n")
            except UnicodeDecodeError as e:
                raise SourceError(f"{path}: line {number}: not UTF-8 ({e})") from None


def read_tatoeba(path: Path) -> Iterator[Article]:
    """Tatoeba の言語ごとの書き出し（`ID<TAB>言語<TAB>文`）。文どうしにつながりはないので、1 文を 1 記事にする。"""
    for number, line in _lines(path):
        if not line:
            continue
        fields = line.split("\t")
        if len(fields) != 3 or not fields[0].isdigit():
            raise SourceError(f"{path}: line {number}: expected 'id<TAB>lang<TAB>text'")
        sentences = split_sentences(fields[2])
        if fields[1] == "jpn" and sentences:
            yield Article(f"tatoeba:{fields[0]}", "", sentences)


def read_common_voice(path: Path) -> Iterator[Article]:
    """Common Voice の文の一覧（1 行に 1 文）。1 文を 1 記事にする。"""
    for number, line in _lines(path):
        sentences = split_sentences(line)
        if sentences:
            yield Article(f"commonvoice:{path.stem}:{number}", "", sentences)


def read_aozora(path: Path) -> Iterator[Article]:
    """青空文庫を整えたデータ（globis-university/aozorabunko-clean、1 行に 1 作品の JSON）。

    今の表記に近い「新字新仮名」の作品で、著作権の切れたものだけを使う。
    """
    for number, line in _lines(path):
        if not line:
            continue
        try:
            row = json.loads(line)
            text, meta = row["text"], row["meta"]
            work = str(meta["作品ID"])
        except (json.JSONDecodeError, KeyError, TypeError) as e:
            raise SourceError(f"{path}: line {number}: expected {{'text', 'meta': {{'作品ID', ...}}}} ({e!r})") from None
        if meta.get("文字遣い種別") != "新字新仮名" or meta.get("作品著作権フラグ") != "なし":
            continue
        sentences = split_sentences(text)
        if sentences:
            yield Article(f"aozora:{work}", meta.get("作品名", ""), sentences)


def read_web(path: Path, limit: int | None = None) -> Iterator[Article]:
    """Web から集めたコーパス（FineWeb-2 の Parquet。列 `id` と `text`）。1 文書を 1 記事にする。

    文の終わりの記号（[`SENTENCE_FINAL`]）で終わる文だけを残す（メニューや見出しの行を除く）。
    """
    import pyarrow.parquet as pq

    try:
        file = pq.ParquetFile(path)
    except Exception as e:  # pyarrow は壊れたファイルにいろいろな例外を出す
        raise SourceError(f"{path}: not a Parquet file ({e})") from None
    missing = {"id", "text"} - set(file.schema_arrow.names)
    if missing:
        raise SourceError(f"{path}: missing column(s): {', '.join(sorted(missing))} (needs 'id' and 'text')")
    n = 0
    for batch in file.iter_batches(columns=["id", "text"], batch_size=1024):
        for doc, text in zip(batch.column("id").to_pylist(), batch.column("text").to_pylist()):
            if limit is not None and n >= limit:
                return
            sentences = [s for s in split_sentences(text or "") if s.endswith(SENTENCE_FINAL)]
            if sentences:
                n += 1
                yield Article(f"web:{doc}", "", sentences)


READERS = {
    "tatoeba": read_tatoeba,
    "common-voice": read_common_voice,
    "aozora": read_aozora,
    "web": read_web,
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("kind", choices=sorted(READERS))
    parser.add_argument("--in", dest="inputs", type=Path, nargs="+", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--limit", type=int,
                        help="記事（文書・作品）をこの数まで取る（ファイルをまたいで数える）。コーパスの割合をそろえるのに使う")
    args = parser.parse_args(argv)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    n = sentences = 0
    with open(args.out, "w", encoding="utf-8") as f:
        for path in args.inputs:
            left = None if args.limit is None else args.limit - n
            if left is not None and left <= 0:
                break
            articles = read_web(path, left) if args.kind == "web" else READERS[args.kind](path)
            for a in articles:
                if left is not None and n >= args.limit:
                    break
                f.write(json.dumps({"id": a.id, "title": a.title, "sentences": a.sentences}, ensure_ascii=False) + "\n")
                n += 1
                sentences += len(a.sentences)
    print(f"wrote {n} articles ({sentences} sentences) to {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
