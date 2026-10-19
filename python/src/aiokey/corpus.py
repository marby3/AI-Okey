"""Wikipedia 日本語版のダンプから記事を取り出し、文に分ける。"""

DUMP_DATE = ""


class CorpusError(Exception):
    """ダンプを読めなかった。"""


def dump_url():
    raise NotImplementedError


def extract_articles(chunks, limit):
    raise NotImplementedError
