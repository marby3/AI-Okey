"""変換の精度を測る。"""


class ExamplesError(Exception):
    pass


def romaji_keys(reading):
    raise NotImplementedError


def summarize(examples, k):
    raise NotImplementedError


def read_examples(path):
    raise NotImplementedError
