"""Wikipedia 日本語版のダンプから記事を取り出し、文に分ける。

ダンプ（約 4.9GB）は全部は取らない。先頭からストリームで展開し、指定した数の記事を取り出したところで止める。

    python -m aiokey.corpus --limit 2000 --out data/wiki/articles.jsonl
"""

from __future__ import annotations

import argparse
import bz2
import json
import re
import sys
import urllib.request
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Iterator

import mwparserfromhell

# 取得するダンプの日付（固定）。Wikimedia は古いダンプを消すので、取れなくなったら新しい日付に上げる。
DUMP_DATE = "20261001"

# 1 文の長さ（文字数）の範囲。短すぎる見出しや、長すぎて切れ目の怪しい文を外す。
MIN_SENTENCE = 4
MAX_SENTENCE = 80

SENTENCE_END = re.compile(r"(?<=[。！？!?])")
# 本文として残さないリンク（画像やカテゴリー）の名前空間
DROP_LINK = re.compile(r"^\s*:?\s*(file|image|ファイル|画像|category|カテゴリ)\s*:", re.I)
# 中身ごと消すタグ
DROP_TAGS = {"ref", "references", "gallery", "math", "chem", "source", "syntaxhighlight", "pre", "score", "timeline"}
# 日本語（かな・漢字）を 1 文字も含まない文は外す
JAPANESE = re.compile(r"[\u3041-\u30ff\u4e00-\u9fff]")


class CorpusError(Exception):
    """ダンプを読めなかった（壊れている、途中で切れている）。"""


@dataclass
class Article:
    id: str
    title: str
    sentences: list[str]


def dump_url(date: str = DUMP_DATE) -> str:
    return (
        f"https://dumps.wikimedia.org/jawiki/{date}/"
        f"jawiki-{date}-pages-articles-multistream.xml.bz2"
    )


class _Feeder:
    """展開した XML を、ElementTree の iterparse に渡すためのファイル風のもの。"""

    def __init__(self, chunks: Iterable[bytes]):
        self._it = _decompress_tracked(chunks)
        self._buf = b""

    def read(self, size: int = -1) -> bytes:
        while size < 0 or len(self._buf) < size:
            try:
                self._buf += next(self._it)
            except StopIteration:
                break
        if size < 0:
            out, self._buf = self._buf, b""
        else:
            out, self._buf = self._buf[:size], self._buf[size:]
        return out


def _decompress_tracked(chunks: Iterable[bytes]) -> Iterator[bytes]:
    """bz2 のストリーム（multistream なので複数つながっている）を展開する。
    最後のストリームが閉じずに終わったら、途中で切れているのでエラーにする。"""
    decompressor = bz2.BZ2Decompressor()
    fed = False
    for chunk in chunks:
        while chunk:
            fed = True
            try:
                yield decompressor.decompress(chunk)
            except OSError as e:
                raise CorpusError(f"the dump is not valid bzip2: {e}") from e
            if decompressor.eof:
                chunk = decompressor.unused_data
                decompressor = bz2.BZ2Decompressor()
                fed = False
            else:
                chunk = b""
    if fed and not decompressor.eof:
        raise CorpusError("the dump ends in the middle of a bzip2 stream")


def clean_wikitext(text: str) -> str:
    """wiki の構文（リンク、テンプレート、強調、表など）を外して、本文の文字だけにする。"""
    code = mwparserfromhell.parse(text)
    for link in code.filter_wikilinks(recursive=False):
        if DROP_LINK.match(str(link.title)):
            _remove(code, link)
    for tag in code.filter_tags(recursive=False):
        if str(tag.tag).strip().lower() in DROP_TAGS:
            _remove(code, tag)
    plain = code.strip_code(normalize=True, collapse=True)
    # 残りがちな記号や、見出しの = を落とす
    plain = re.sub(r"^=+.*?=+\s*$", "", plain, flags=re.M)
    plain = re.sub(r"[\[\]{}|]", "", plain)
    return plain


def _remove(code, node) -> None:
    try:
        code.remove(node)
    except ValueError:
        pass


def split_sentences(text: str) -> list[str]:
    """本文を文に分ける。改行と、。！？で区切り、長さが範囲外の文は外す。"""
    out = []
    for line in text.splitlines():
        for sentence in SENTENCE_END.split(line.strip()):
            sentence = sentence.strip()
            if MIN_SENTENCE <= len(sentence) <= MAX_SENTENCE and JAPANESE.search(sentence):
                out.append(sentence)
    return out


def _local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def extract_articles(chunks: Iterable[bytes], limit: int) -> Iterator[Article]:
    """bz2 圧縮されたダンプのバイト列から、標準名前空間の記事（転送ページを除く）を `limit` 件まで取り出す。"""
    count = 0
    try:
        for _, elem in ET.iterparse(_Feeder(chunks), events=("end",)):
            if _local(elem.tag) != "page":
                continue
            fields = {_local(child.tag): child for child in elem}
            ns = fields.get("ns")
            is_redirect = "redirect" in fields
            if ns is not None and ns.text == "0" and not is_redirect:
                text_elem = next((e for e in elem.iter() if _local(e.tag) == "text"), None)
                sentences = split_sentences(clean_wikitext(text_elem.text or "")) if text_elem is not None else []
                if sentences:
                    yield Article(
                        id=fields["id"].text or "",
                        title=fields["title"].text or "",
                        sentences=sentences,
                    )
                    count += 1
            elem.clear()
            if count >= limit:
                return
    except ET.ParseError as e:
        raise CorpusError(f"the dump is not valid XML: {e}") from e


# Wikimedia は User-Agent のない取得を拒否する（https://meta.wikimedia.org/wiki/User-Agent_policy）
USER_AGENT = "AI-O-Key-datagen/0.6 (https://github.com/marby3/AI-Okey)"


def _http_chunks(url: str, size: int = 1 << 20) -> Iterator[bytes]:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request) as response:
        while chunk := response.read(size):
            yield chunk


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--limit", type=int, default=2000, help="取り出す記事の数")
    parser.add_argument("--out", type=Path, default=Path("data/wiki/articles.jsonl"))
    parser.add_argument("--date", default=DUMP_DATE, help="ダンプの日付（YYYYMMDD）")
    args = parser.parse_args(argv)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    n = 0
    with args.out.open("w", encoding="utf-8") as f:
        for article in extract_articles(_http_chunks(dump_url(args.date)), args.limit):
            f.write(json.dumps(article.__dict__, ensure_ascii=False) + "\n")
            n += 1
    print(f"wrote {n} articles from jawiki-{args.date} to {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
