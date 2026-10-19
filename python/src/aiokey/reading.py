"""Sudachi で読みを付け、入力（スケルトン）を作る。

英字だけのトークン（英単語・略語）と、半角の数字・記号・空白は、読みにせずそのまま入力に残す
（AI-O Key では英単語をつづりどおりに打ち、数字や記号もそのまま打つため）。
"""

from __future__ import annotations

import re
from dataclasses import asdict, dataclass

from sudachipy import Dictionary, SplitMode

from aiokey import skeletonize, units

ASCII_WORD = re.compile(r"[A-Za-z][A-Za-z-]*")
ASCII_DIGITS = re.compile(r"[0-9]+")


@dataclass
class Token:
    surface: str
    # かなの読み（英単語・数字はつづりのまま）
    reading: str
    # 英字だけのトークンか
    english: bool

    def to_dict(self) -> dict:
        return asdict(self)


class Annotator:
    """文をトークンに分けて読みを付ける。Sudachi の辞書の読み込みに時間がかかるので、1 つを使い回す。"""

    def __init__(self) -> None:
        self._tokenizer = Dictionary(dict="core").tokenizer(mode=SplitMode.C)

    def tokens(self, sentence: str) -> list[Token]:
        out = []
        for m in self._tokenizer.tokenize(sentence):
            surface = m.surface()
            if ASCII_WORD.fullmatch(surface):
                out.append(Token(surface, surface, True))
            elif ASCII_DIGITS.fullmatch(surface) or m.part_of_speech()[0] in ("補助記号", "空白"):
                # 数字・記号・空白は読みにせず、そのまま打つ
                out.append(Token(surface, surface, False))
            else:
                out.append(Token(surface, _hiragana(m.reading_form()), False))
        return out


def _hiragana(katakana: str) -> str:
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in katakana)


def token_input(token: Token, keep_vowels: set[int] | None = None, start: int = 0) -> str:
    """1 トークンぶんの入力。`keep_vowels` に入っている位置（文全体のかなの通し番号）は、子音に母音を付けて打つ。"""
    if token.english:
        return token.reading.lower()
    if not keep_vowels:
        return skeletonize(token.reading)
    out = []
    for i, (letter, vowel) in enumerate(units(token.reading)):
        out.append(letter)
        if start + i in keep_vowels and vowel and letter not in "aiueoqx-" and letter.isascii():
            out.append(vowel)
    return "".join(out)


def unit_count(token: Token) -> int:
    """入力の単位（スケルトンの文字）の数。英単語は母音を残さないので 0 と数える。"""
    return 0 if token.english else len(units(token.reading))


def to_input(tokens: list[Token], keep_vowels: set[int] | None = None) -> str:
    """トークンの並びから、打つ入力を作る。"""
    out = []
    position = 0
    for token in tokens:
        out.append(token_input(token, keep_vowels, position))
        position += unit_count(token)
    return "".join(out)
