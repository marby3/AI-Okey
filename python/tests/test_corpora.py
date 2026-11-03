"""Issue #41 の受け入れテスト（基準 1〜4）：学習データを増やし、ほかのコーパスを加え、評価の文を除く。

記録は docs/training-data.md。基準 5 は test_train_prep.py、基準 6・7 は tests/repo/test_training_plan.py。
"""

import bz2
import gzip
import json
import os
import re
import subprocess
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

import aiokey
from aiokey import datagen, sources

ROOT = Path(__file__).resolve().parents[2]
MOZC = Path(os.environ.get("AIOKEY_MOZC_DIR", ROOT / "data/mozc"))
DOC = ROOT / "docs/training-data.md"


@pytest.fixture(scope="module")
def dict_path(tmp_path_factory):
    path = tmp_path_factory.mktemp("dict") / "aiokey.dic"
    aiokey.build_dictionary(str(MOZC), str(path))
    return path


def write_articles(path, articles):
    path.write_text(
        "".join(json.dumps({"id": i, "title": i, "sentences": s}, ensure_ascii=False) + "\n" for i, s in articles),
        encoding="utf-8",
    )
    return path


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]


def table_row(text, first_cell):
    line = next(l for l in text.splitlines() if l.startswith(f"| {first_cell}"))
    return [c.strip() for c in line.strip().strip("|").split("|")]


def test_wikipedia_grows_past_20000_articles_and_test_articles_stay_out_of_training(tmp_path, dict_path):
    # 基準 1：記録した Wikipedia の記事が 2 万以上。いくつのソースを並列に混ぜても、test の記事は train / dev に入らない
    row = table_row(DOC.read_text(encoding="utf-8"), "Wikipedia")
    assert int(row[1].replace(",", "")) >= 20000, row
    # test の記事の文はほかの記事でも使わないので、記事ごとに違う文にする
    wiki = write_articles(tmp_path / "wiki.jsonl", [(f"{i}", [f"果たして{i}回目は本当に。"]) for i in range(120)])
    other = write_articles(tmp_path / "tatoeba.jsonl", [(f"tatoeba:{i}", [f"今日は{i}日で天気が良い。"]) for i in range(120)])
    out = tmp_path / "examples"
    datagen.main(["--articles", str(wiki), str(other), "--dict", str(dict_path), "--out", str(out),
                  "--splits", "train,dev", "--workers", "2"])
    seen = [e["article"] for name in ("train", "dev") for e in read_jsonl(out / f"{name}.jsonl")]
    assert {a.startswith("tatoeba:") for a in seen} == {True, False}
    assert all(datagen.split_of(a) != "test" for a in seen)
    assert not (out / "test.jsonl").exists()


def test_clearly_licensed_corpora_are_read_and_their_decisions_recorded(tmp_path):
    # 基準 2：Tatoeba・Common Voice・青空文庫を記事の形にし、候補ごとの判断とライセンス、取得するファイルのハッシュを残す
    tsv = tmp_path / "jpn_sentences.tsv.bz2"
    tsv.write_bytes(bz2.compress("1\tjpn\t今日は雨です。\n2\tjpn\t明日は晴れるでしょう。\n".encode()))
    assert [(a.id, a.sentences) for a in sources.read_tatoeba(tsv)] == [
        ("tatoeba:1", ["今日は雨です。"]),
        ("tatoeba:2", ["明日は晴れるでしょう。"]),
    ]
    cv = tmp_path / "sentence-collector.txt"
    cv.write_text("FirefoxはMozillaが開発したウェブブラウザーです。\n\n「はい、出来ます。」と、少年は答えました。\n", encoding="utf-8")
    assert [a.sentences for a in sources.read_common_voice(cv)] == [
        ["FirefoxはMozillaが開発したウェブブラウザーです。"],
        ["「はい、出来ます。」と、少年は答えました。"],
    ]
    aozora = tmp_path / "aozora.jsonl.gz"
    rows = [
        {"text": "吾輩は猫である。名前はまだ無い。", "meta": {"作品ID": "1", "文字遣い種別": "新字新仮名", "作品著作権フラグ": "なし"}},
        {"text": "舊字の作品である。", "meta": {"作品ID": "2", "文字遣い種別": "旧字旧仮名", "作品著作権フラグ": "なし"}},
        {"text": "著作権の残る作品である。", "meta": {"作品ID": "3", "文字遣い種別": "新字新仮名", "作品著作権フラグ": "あり"}},
    ]
    aozora.write_bytes(gzip.compress("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows).encode()))
    assert [(a.id, a.sentences) for a in sources.read_aozora(aozora)] == [
        ("aozora:1", ["吾輩は猫である。", "名前はまだ無い。"])
    ]
    doc = DOC.read_text(encoding="utf-8")
    for name, license_ in [("Tatoeba", "CC BY 2.0 FR"), ("Common Voice", "CC0"), ("青空文庫", "CC BY 4.0"),
                           ("Wikinews", "CC BY")]:
        row = table_row(doc, name)
        assert row[1] in {"使う", "使わない"} and license_ in " ".join(row), row
    pinned = (ROOT / "scripts/corpora.sha256").read_text(encoding="utf-8")
    for file in ["jpn_sentences.tsv.bz2", "sentence-collector.txt", "aozorabunko-dedupe-clean.jsonl.gz"]:
        assert re.search(rf"^[0-9a-f]{{64}}  \S*{re.escape(file)}$", pinned, re.M), file


def test_web_corpus_has_its_legal_basis_recorded_and_never_enters_the_repo_or_releases(tmp_path):
    # 基準 3：Web のコーパス（Parquet）を記事の形にする。法的な根拠を記録し、データはリポジトリにも配布物にも入れない
    parquet = tmp_path / "000_00000.parquet"
    pq.write_table(pa.table({"id": ["<urn:uuid:a>"], "text": ["今日はいい天気ですね。散歩に行きましょう！"],
                             "url": ["https://example.com/"]}), parquet)
    assert [(a.id, a.sentences) for a in sources.read_web(parquet)] == [
        ("web:<urn:uuid:a>", ["今日はいい天気ですね。", "散歩に行きましょう！"])
    ]
    doc = DOC.read_text(encoding="utf-8")
    start = doc.index("## Web のコーパスを使う法的な根拠")
    legal = doc[start : doc.find("\n## ", start + 1)]
    for item in ["著作権法", "第三十条の四", "情報解析", "著作権者の利益を不当に害する", "ただし書き", "ODC-By",
                 "Common Crawl", "リポジトリ", "配布物"]:
        assert item in legal, item
    for path in ["data/corpora/fineweb-2/000_00000.parquet", "data/corpora/articles/web.jsonl",
                 "target/examples/train.jsonl"]:
        # コンテナの中ではリポジトリの持ち主が違うので、safe.directory を付ける
        assert subprocess.run(["git", "-c", "safe.directory=*", "check-ignore", "-q", path], cwd=ROOT).returncode == 0, path
    # 配布物に入るファイル（Release の Copy-Item と gh release、インストーラーの Source:）は、決めた一覧のものだけ
    allowed = {"aiokey.dll", "aiokey-server.exe", "aiokey-ui.exe", "aiokey-launcher.exe", "aiokey-cli.exe", "LICENSE",
               "NOTICE", "aiokey.dic", "aiokey-stage2-q8_0.gguf", "aiokey-stage2.json", "README.txt", "register.ps1",
               "MOZC-DICTIONARY-README.txt", "dxwebsetup.exe"}
    release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    shipped = [s.strip().strip('"') for line in re.findall(r"Copy-Item (.+?) (?:\$out|\"\$out)", release)
               for s in line.split(",")]
    create = re.search(r"gh release create (.+?)--title", release, re.S).group(1)
    shipped += [s.strip('"') for s in create.replace("\\\n", " ").split()[1:] if "$GITHUB_REF_NAME" not in s]
    for iss in (ROOT / "installer").glob("*.iss"):
        shipped += re.findall(r'^Source: "[^"]*?([^"/}]+)"', iss.read_text(encoding="utf-8", errors="ignore"), re.M)
    assert shipped and {Path(s).name for s in shipped} <= allowed, sorted({Path(s).name for s in shipped} - allowed)


def test_evaluation_sentences_are_removed_from_training_data(tmp_path, dict_path):
    # 基準 4：評価の例（test セット、ATC 2026、固有名詞）の正解と同じ文は、学習の文にも文脈にも使わない
    # 除く文は test でない記事にだけ置く（test の記事の文は、それだけで train / dev から除かれるので）
    articles = write_articles(tmp_path / "a.jsonl", [
        (f"{i}", (["今日は雨です。"] if datagen.split_of(f"{i}") != "test" else []) + [f"果たして{i}回目は本当に。"])
        for i in range(60)
    ])
    atc = tmp_path / "atc.jsonl"
    atc.write_text(json.dumps({"target": "今日は雨です。"}, ensure_ascii=False) + "\n", encoding="utf-8")
    out = tmp_path / "examples"
    datagen.main(["--articles", str(articles), "--dict", str(dict_path), "--out", str(out),
                  "--splits", "train,dev", "--exclude", str(atc), "--context-rate", "1"])
    examples = [e for name in ("train", "dev") for e in read_jsonl(out / f"{name}.jsonl")]
    assert examples
    assert all("今日は雨です" not in e["target"] and "今日は雨です" not in e["context"] for e in examples)


def test_malformed_tatoeba_export_is_an_error_with_the_line_number(tmp_path):
    path = tmp_path / "jpn_sentences.tsv"
    path.write_text("1\tjpn\t今日は雨です。\n2\tjpn\n", encoding="utf-8")
    with pytest.raises(sources.SourceError, match="line 2"):
        list(sources.read_tatoeba(path))


def test_malformed_common_voice_text_is_an_error_with_the_line_number(tmp_path):
    path = tmp_path / "sentence-collector.txt"
    path.write_bytes("今日は雨です。\n".encode() + b"\xff\xfe\n")
    with pytest.raises(sources.SourceError, match="line 2"):
        list(sources.read_common_voice(path))


def test_malformed_aozora_row_is_an_error_with_the_line_number(tmp_path):
    path = tmp_path / "aozora.jsonl"
    path.write_text(json.dumps({"meta": {"作品ID": "1"}}, ensure_ascii=False) + "\n", encoding="utf-8")
    with pytest.raises(sources.SourceError, match="line 1"):
        list(sources.read_aozora(path))


def test_web_corpus_without_a_text_column_is_an_error(tmp_path):
    path = tmp_path / "broken.parquet"
    pq.write_table(pa.table({"id": ["a"], "body": ["今日は雨です。"]}), path)
    with pytest.raises(sources.SourceError, match="text"):
        list(sources.read_web(path))
