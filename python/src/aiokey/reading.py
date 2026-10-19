"""Sudachi で読みを付け、入力（スケルトン）を作る。"""


class Annotator:
    def tokens(self, sentence):
        raise NotImplementedError


def to_input(tokens):
    raise NotImplementedError
