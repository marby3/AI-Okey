"""aiokey のコマンド。

    python -m aiokey eval --examples data/examples/test.jsonl --dict build/aiokey.dic [-k 32]
    python -m aiokey train --train data/examples/train.jsonl --dev data/examples/dev.jsonl --out build/stage2
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    # 学習は torch を読み込むので、train のときだけ aiokey.train に任せる（引数もそちらで解釈する）
    if argv[:1] == ["train"]:
        from aiokey import train

        return train.main(argv[1:])

    parser = argparse.ArgumentParser(prog="aiokey")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("train", help="段2を学習する（引数は aiokey train --help）")
    ev = sub.add_parser("eval", help="変換の精度を測り、JSON で出す")
    ev.add_argument("--examples", type=Path, required=True, help="aiokey.datagen が作った JSONL")
    ev.add_argument("--dict", type=Path, default=Path("build/aiokey.dic"))
    ev.add_argument("-k", type=int, default=32)
    ev.add_argument("--scorer", type=Path, help="段2の GGUF。渡すと段1の候補を段2で並べ替えてから数える")
    ev.add_argument("--format", choices=["zenz", "aiokey"], default="zenz", help="段2のプロンプトの形式")
    ev.add_argument("--threads", type=int, default=2, help="段2の推論のスレッド数")
    ev.add_argument("--limit", type=int, help="先頭からこの数の例だけで測る")
    ev.add_argument("--max-target-chars", type=int, help="正解の文字数がこれ以下の例だけで測る")
    args = parser.parse_args(argv)

    import itertools

    from aiokey import Converter, Scorer
    from aiokey.evaluate import ExamplesError, evaluate, read_examples

    if args.k < 1:
        parser.error("-k must be at least 1")
    try:
        scorer = Scorer(str(args.scorer), args.format, args.threads, args.k) if args.scorer else None
        examples = read_examples(args.examples)
        if args.max_target_chars:
            examples = (e for e in examples if len(e["target"]) <= args.max_target_chars)
        examples = itertools.islice(examples, args.limit)
        report = evaluate(examples, Converter(str(args.dict)), args.k, scorer)
    except (ExamplesError, OSError, ValueError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
