"""aiokey のコマンド。

    python -m aiokey eval --examples data/examples/test.jsonl --dict build/aiokey.dic [-k 32]
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="aiokey")
    sub = parser.add_subparsers(dest="command", required=True)
    ev = sub.add_parser("eval", help="変換の精度を測り、JSON で出す")
    ev.add_argument("--examples", type=Path, required=True, help="aiokey.datagen が作った JSONL")
    ev.add_argument("--dict", type=Path, default=Path("build/aiokey.dic"))
    ev.add_argument("-k", type=int, default=32)
    args = parser.parse_args(argv)

    from aiokey import Converter
    from aiokey.evaluate import ExamplesError, evaluate, read_examples

    if args.k < 1:
        parser.error("-k must be at least 1")
    try:
        report = evaluate(read_examples(args.examples), Converter(str(args.dict)), args.k)
    except ExamplesError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
