"""aiokey のコマンド。

    python -m aiokey eval --examples data/examples/test.jsonl --dict build/aiokey.dic [-k 32]
    python -m aiokey sentences --text data/atc/atc26.txt --set atc --out target/eval/atc.jsonl
    python -m aiokey analyze --examples target/eval/atc.jsonl --dict build/aiokey.dic [--scorer GGUF]
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
    # train は上で aiokey.train に渡しているので、ここでは aiokey --help に載せるためだけに登録する
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
    se = sub.add_parser("sentences", help="ふつうの文から評価の例（文・句、子音だけ・母音あり）を作る")
    src = se.add_mutually_exclusive_group(required=True)
    src.add_argument("--text", type=Path, help="1 行に 1 つ以上の文を書いたテキスト")
    src.add_argument("--examples", type=Path, help="aiokey.datagen の例（途中で切っていない文の正解を使う）")
    se.add_argument("--set", required=True, help="例に付ける評価セットの名前")
    se.add_argument("--limit", type=int, help="先頭からこの数の行（文）だけを使う")
    se.add_argument("--out", type=Path, required=True)
    an = sub.add_parser("analyze", help="誤りの内訳と、測り方ごとの数字を JSON で出す")
    an.add_argument("--examples", type=Path, required=True, help="sentences が作った JSONL")
    an.add_argument("--dict", type=Path, default=Path("build/aiokey.dic"))
    an.add_argument("--scorer", type=Path, help="段2の GGUF。渡すとサーバーと同じく迷うときだけ段2で並べ替える")
    an.add_argument("--temperature", type=float,
                    help="段2の温度。省くと GGUF と同じフォルダの aiokey-stage2.json の temperature")
    an.add_argument("--threads", type=int, default=2)
    an.add_argument("--rows", type=Path, help="例ごとの結果（JSONL）を書く先")
    args = parser.parse_args(argv)

    if args.command == "sentences":
        return _sentences(args)
    if args.command == "analyze":
        return _analyze(args)

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


def _sentences(args) -> int:
    import itertools

    from aiokey import reading as rd
    from aiokey.analyze import make_examples, read_example_targets, read_texts

    texts = read_texts(args.text) if args.text else read_example_targets(args.examples)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        for e in make_examples(itertools.islice(texts, args.limit), args.set, rd.Annotator()):
            f.write(json.dumps(e, ensure_ascii=False) + "\n")
    return 0


def _analyze(args) -> int:
    from aiokey import Converter, Scorer
    from aiokey.analyze import MAX_CANDIDATES, Analyzer, analyze

    scorer = None
    if args.scorer:
        temperature = args.temperature
        if temperature is None:
            with open(args.scorer.parent / "aiokey-stage2.json", encoding="utf-8") as f:
                temperature = json.load(f)["temperature"]
        scorer = Scorer(str(args.scorer), "aiokey", args.threads, MAX_CANDIDATES, temperature)
    with open(args.examples, encoding="utf-8") as f:
        examples = [json.loads(line) for line in f if line.strip()]
    report, rows = analyze(examples, Analyzer(Converter(str(args.dict)), scorer))
    if args.rows:
        with open(args.rows, "w", encoding="utf-8") as f:
            for row in rows:
                f.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
