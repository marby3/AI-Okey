"""変換の誤りを分析し、測り方ごとの数字を出す（#39）。

    python -m aiokey sentences --text data/atc/atc26.txt --set atc --out target/eval/atc.jsonl
    python -m aiokey sentences --examples target/examples/test.jsonl --set test --out target/eval/test.jsonl
    python -m aiokey analyze --examples target/eval/atc.jsonl --dict build/aiokey.dic \
        --scorer data/stage2/aiokey-stage2-q8_0.gguf --temperature 3.44

`sentences` は、ふつうの文（1 行に 1 つ以上の文）から評価の例を作る。文（「。」で区切る）と句（「、」「。」で
区切る）の 2 つの単位で、子音だけの入力と、母音をいくつか足した入力の両方を作る。

`analyze` は、例をサーバーと同じ手順で変換する（段1の 1 位と 2 位のコスト差が `skip_cost_gap` 以上なら段1の
並びのまま、迷うときは段1の上位 `max_candidates` 件を段2で並べ替える）。結果を JSON で出す：

- measures：単位（文・句）と入力（子音だけ・母音あり）ごとの完全一致（1 位）、候補ウィンドウの 1 ページ目
  （9 件）、段1の候補（K 件）に入る割合、1 位の文字の一致率
- errors：誤りを「候補に正解がない」「候補にあるが 1 位でない」に分け、原因（辞書にない語、固有名詞・カタカナ語、
  数字、英字、読みの違い・表記の違い）ごとの割合と、文の長さごとの誤りの割合

`compare` は、段2で文を生成する方式（#40 の試作）と今の方式を、同じ例で比べる（`compare()`）。
"""

from __future__ import annotations

import json
import random
import re
import statistics
import time
from collections import Counter
from pathlib import Path
from typing import Iterable, Iterator

from aiokey import reading as rd
from aiokey.datagen import VOWEL_PER_KANA

PAGE = 9
# サーバーの既定（crates/aiokey-server/src/config.rs）と同じ
K = 32
SKIP_COST_GAP = 500
MAX_CANDIDATES = 8
SENTENCE_END = re.compile(r"(?<=。)")
PHRASE_END = re.compile(r"(?<=[、。])")
KATAKANA_WORD = re.compile(r"[ァ-ヶー・]{2,}")
DIGIT = re.compile(r"[0-9０-９]")
SYMBOL = re.compile(r"[^\w・ー]")
# 単独の入力で候補に出るかを見るときの K
WORD_K = 200
FEATURES = ("辞書にない語", "固有名詞・カタカナ語", "数字", "英字")
LENGTHS = ((10, "10 文字以下"), (20, "11〜20 文字"), (None, "21 文字以上"))


def split(text: str, unit: str) -> list[str]:
    pattern = SENTENCE_END if unit == "sentence" else PHRASE_END
    return [s for s in pattern.split(text.strip()) if s.strip()]


def to_input(tokens: list[rd.Token], keep: set[int]) -> str:
    """実際に打つ入力。英字はつづり（大文字も）のまま、「・」は `/` で打つ（v1.0.1）。"""
    out = []
    position = 0
    for token in tokens:
        out.append(token.reading if token.english else rd.token_input(token, keep, position))
        position += rd.unit_count(token)
    return "".join(out).replace("・", "/")


def make_examples(
    texts: Iterable[tuple[str, str]], set_name: str, annotator: rd.Annotator, seed: int = 0
) -> Iterator[dict]:
    """(ID, 文章) の並びから例を作る。文脈は、同じ文章の中でその単位より前の部分。"""
    rng = random.Random(seed)
    for text_id, text in texts:
        for unit in ("sentence", "phrase"):
            context = ""
            for target in split(text, unit):
                tokens = annotator.tokens(target)
                total = sum(rd.unit_count(t) for t in tokens)
                keep = {i for i in range(total) if rng.random() < VOWEL_PER_KANA}
                if total and not keep:
                    keep = {rng.randrange(total)}
                for vowels in (False, True):
                    yield {
                        "set": set_name,
                        "id": text_id,
                        "unit": unit,
                        "context": context,
                        "input": to_input(tokens, keep if vowels else set()),
                        "target": target,
                        "reading": "".join(t.reading for t in tokens),
                        "tokens": [t.to_dict() for t in tokens],
                        "vowels_kept": vowels,
                    }
                context += target


def read_texts(path: Path) -> Iterator[tuple[str, str]]:
    with open(path, encoding="utf-8") as f:
        for no, line in enumerate(f, 1):
            if line.strip():
                yield str(no), line.strip()


def read_example_targets(path: Path) -> Iterator[tuple[str, str]]:
    """aiokey.datagen の例（JSONL）から、途中で切っていない文の正解を 1 つずつ取り出す（同じ文は 1 回）。"""
    seen = set()
    with open(path, encoding="utf-8") as f:
        for line in f:
            e = json.loads(line)
            key = (e["article"], e["sentence_index"])
            if e.get("truncated") or key in seen:
                continue
            seen.add(key)
            yield f"{e['article']}:{e['sentence_index']}", e["target"]


def hiragana(text: str) -> str:
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in text)


def char_accuracy(output: str, target: str) -> float:
    """1 −（編集距離 ÷ 正解の文字数）。0 未満は 0。"""
    previous = list(range(len(target) + 1))
    for i, a in enumerate(output, 1):
        current = [i]
        for j, b in enumerate(target, 1):
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (a != b)))
        previous = current
    return max(0.0, 1 - previous[-1] / len(target)) if target else 0.0


class Analyzer:
    def __init__(self, converter, scorer=None, k: int = K, skip_cost_gap: int = SKIP_COST_GAP,
                 max_candidates: int = MAX_CANDIDATES):
        self.converter = converter
        self.scorer = scorer
        self.k = k
        self.skip_cost_gap = skip_cost_gap
        self.max_candidates = max_candidates
        self._word_cache: dict[tuple[str, str], bool] = {}
        from sudachipy import Dictionary, SplitMode

        # reading.Annotator と同じ辞書と分割単位
        self._tokenizer = Dictionary(dict="core").tokenizer(mode=SplitMode.C)

    def rank(self, e: dict) -> dict:
        """サーバーと同じ手順で並べた候補と、正解の順位を返す。"""
        c = self.converter.convert_with_costs(e["input"], self.k)
        order = list(range(len(c)))
        if self.scorer is not None and len(c) >= 2 and c[1][2] - c[0][2] < self.skip_cost_gap:
            n = min(len(c), self.max_candidates)
            probs, _ = self.scorer.score(e.get("context", ""), e["input"], [(s, r) for s, r, _ in c[:n]])
            order = sorted(range(len(probs)), key=lambda i: -probs[i]) + list(range(len(probs), len(c)))
        surfaces = [c[i][0] for i in order]
        target = e["target"]
        return {
            "surfaces": surfaces,
            "top_reading": c[order[0]][1] if c else "",
            "rank": surfaces.index(target) + 1 if target in surfaces else None,
        }

    def word_found(self, token: dict) -> bool:
        """語を単独で打ったとき、段1の候補（WORD_K 件）に出るか。出ないなら辞書にない（か、ほぼ出ない）語。"""
        key = (token["surface"], token["reading"])
        if key not in self._word_cache:
            skeleton = rd.token_input(rd.Token(**token))
            self._word_cache[key] = token["surface"] in self.converter.convert(skeleton, WORD_K)
        return self._word_cache[key]

    def features(self, e: dict) -> list[str]:
        """入力の特徴（誤りの原因の候補）。すべての例で数え、特徴のある例とない例で誤りの割合を比べる。"""
        tokens = e.get("tokens", [])
        out = []
        # 内容語（表記と読みが違う語）だけを見る。ひらがなだけの語（助詞など）と、数字・記号を含む語
        # （Sudachi は「6600万」「%」にも読みを付ける）は除く。数字は「数字」で別に数える
        words = [t for t in tokens if not t["english"] and t["surface"] != t["reading"]
                 and not DIGIT.search(t["surface"]) and not SYMBOL.search(t["surface"])]
        if any(not self.word_found(t) for t in words):
            out.append("辞書にない語")
        proper = any(m.part_of_speech()[1] == "固有名詞" for m in self._tokenizer.tokenize(e["target"]))
        if proper or any(KATAKANA_WORD.fullmatch(t["surface"]) for t in tokens):
            out.append("固有名詞・カタカナ語")
        if DIGIT.search(e["target"]):
            out.append("数字")
        if any(t["english"] for t in tokens):
            out.append("英字")
        return out


def mistake(e: dict, result: dict) -> str:
    """1 位の読みが正解と同じなら表記（漢字の選び方）の違い、違えば読み（スケルトンの解釈）の違い。"""
    same = hiragana(result["top_reading"]) == hiragana(e["reading"])
    return "表記の違い" if same else "読みの違い"


def length_label(n: int) -> str:
    return next(label for limit, label in LENGTHS if limit is None or n <= limit)


def analyze(examples: Iterable[dict], analyzer: Analyzer) -> tuple[dict, list[dict]]:
    """指標と誤りの内訳、例ごとの結果（誤りの見積もりに使う）を返す。"""
    measures: dict[str, Counter] = {}
    errors = {"候補に正解がない": Counter(), "候補にあるが 1 位でない": Counter()}
    lengths: dict[str, Counter] = {}
    by_feature: dict[str, Counter] = {}
    rows = []
    for e in examples:
        result = analyzer.rank(e)
        rank = result["rank"]
        stage1 = rank is not None
        key = f"{e.get('unit', 'sentence')}/{'vowels' if e.get('vowels_kept') else 'consonants'}"
        m = measures.setdefault(key, Counter())
        m["examples"] += 1
        m["top1"] += rank == 1
        m["page1"] += rank is not None and rank <= PAGE
        m["in_candidates"] += stage1
        m["char_accuracy"] += char_accuracy(result["surfaces"][0] if result["surfaces"] else "", e["target"])
        length = lengths.setdefault(length_label(len(e["target"])), Counter())
        length["examples"] += 1
        length["errors"] += rank != 1
        features = analyzer.features(e)
        for feature in FEATURES:
            f = by_feature.setdefault(feature, Counter())
            has = feature in features
            f["with" if has else "without"] += 1
            f[("with" if has else "without") + "_errors"] += rank != 1
        if rank != 1:
            status = "候補にあるが 1 位でない" if stage1 else "候補に正解がない"
            errors[status]["examples"] += 1
            for cause in features + ([mistake(e, result)] if result["surfaces"] else []):
                errors[status][cause] += 1
        rows.append({**{k: e.get(k) for k in ("set", "id", "unit", "context", "input", "target", "vowels_kept")},
                     "rank": rank, "features": features, "top": result["surfaces"][:PAGE]})
    report = {
        "measures": {
            key: {
                "examples": m["examples"],
                "top1": m["top1"] / m["examples"],
                "page1": m["page1"] / m["examples"],
                "in_candidates": m["in_candidates"] / m["examples"],
                "char_accuracy": m["char_accuracy"] / m["examples"],
            }
            for key, m in sorted(measures.items())
        },
        "errors": {
            status: {"examples": c["examples"],
                     "causes": {k: v / c["examples"] for k, v in c.items() if k != "examples"}}
            for status, c in errors.items()
        },
        "error_rate_by_feature": {
            k: {"examples": c["with"], "error_rate": c["with_errors"] / c["with"] if c["with"] else 0.0,
                "error_rate_without": c["without_errors"] / c["without"] if c["without"] else 0.0}
            for k, c in by_feature.items()
        },
        "error_rate_by_length": {k: {"examples": c["examples"], "error_rate": c["errors"] / c["examples"]}
                                 for k, c in lengths.items()},
    }
    return report, rows


# #40：比べる方式
METHODS = {"current": "今の方式", "generate": "生成だけ", "combined": "合わせた"}


class Generator:
    """段2で文を生成する方式（#40 の試作）。Analyzer の変換器と採点器を使う。"""

    def __init__(self, analyzer: Analyzer, beam: int = 8, cost_weight: float = 0.5):
        self.analyzer = analyzer
        self.beam = beam
        self.cost_weight = cost_weight

    def generate(self, e: dict) -> list[str]:
        a = self.analyzer
        out = a.scorer.generate(a.converter, e.get("context", ""), e["input"], self.beam, self.cost_weight)
        return [s for s, _ in out]

    def combined(self, e: dict) -> list[str]:
        """段1が迷わないときは今の方式のまま。迷うときは、段1の上位と生成の候補を合わせて段2で並べ、
        残りの段1の候補を後ろにつなぐ。"""
        a = self.analyzer
        c = a.converter.convert_with_costs(e["input"], a.k)
        stage1 = [s for s, _, _ in c]
        if len(c) < 2 or c[1][2] - c[0][2] >= a.skip_cost_gap:
            return stage1
        pool = list(dict.fromkeys(stage1[: a.max_candidates] + self.generate(e)))
        probs, _ = a.scorer.score(e.get("context", ""), e["input"], [(s, s) for s in pool])
        ranked = [pool[i] for i in sorted(range(len(pool)), key=lambda i: -probs[i])]
        return ranked + [s for s in stage1 if s not in pool]

    def candidates(self, e: dict, method: str) -> list[str]:
        if method == "current":
            return self.analyzer.rank(e)["surfaces"]
        if method == "generate":
            return self.generate(e)
        return self.combined(e)


def percentile(values: list[float], q: float) -> float:
    return statistics.quantiles(values, n=100, method="inclusive")[q - 1] if len(values) > 1 else values[0]


def compare(examples: Iterable[dict], generator: Generator, methods: Iterable[str] = METHODS) -> tuple[dict, list[dict]]:
    """方式ごと・単位と入力ごとに、完全一致・1 ページ目・候補に正解がある割合と、1 回の変換の時間（ms）を返す。"""
    methods = list(methods)
    measures: dict[str, dict[str, Counter]] = {m: {} for m in methods}
    times: dict[str, dict[str, list[float]]] = {m: {} for m in methods}
    rows = []
    for e in examples:
        key = f"{e.get('unit', 'sentence')}/{'vowels' if e.get('vowels_kept') else 'consonants'}"
        row = {k: e.get(k) for k in ("set", "id", "unit", "context", "input", "target", "vowels_kept")}
        for method in methods:
            started = time.perf_counter()
            surfaces = generator.candidates(e, method)
            times[method].setdefault(key, []).append((time.perf_counter() - started) * 1000)
            rank = surfaces.index(e["target"]) + 1 if e["target"] in surfaces else None
            m = measures[method].setdefault(key, Counter())
            m["examples"] += 1
            m["top1"] += rank == 1
            m["page1"] += rank is not None and rank <= PAGE
            m["in_candidates"] += rank is not None
            row[method] = {"rank": rank, "top": surfaces[:PAGE]}
        rows.append(row)
    report = {
        method: {
            key: {
                "examples": m["examples"],
                "top1": m["top1"] / m["examples"],
                "page1": m["page1"] / m["examples"],
                "in_candidates": m["in_candidates"] / m["examples"],
                "ms_p50": percentile(times[method][key], 50),
                "ms_p95": percentile(times[method][key], 95),
            }
            for key, m in sorted(measures[method].items())
        }
        for method in methods
    }
    return report, rows
