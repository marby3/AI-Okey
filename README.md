# AI-O Key（あいおーきー）

子音だけで入力することを前提にした日本語 IME です。母音を省いたローマ字（子音スケルトン）を、辞書による候補生成と小型 AI による採点の 2 段で日本語に変換します。

```
htsthqtun  →  果たして本当に
```

- 子音は行ごとに 1 文字（か行は `k`、さ行は `s` …）。ア行の母音はそのまま打ちます。
- 「ん」は `q`、「っ」は `x`、長音は `-` です。
- 曖昧なところだけ母音を足して打てます（`kik` →「聞く」「開花」）。
- 英単語はつづりのまま混ぜて打てます。`/` で始めると英語として扱います。

詳しい仕様は [docs/spec.md](docs/spec.md) を見てください。

> 開発中です。v1.0.0 で Windows（TSF）で実際に打てるようになる予定です。

## 構成

| クレート | 役割 |
|---|---|
| `aiokey-engine` | 変換エンジン（OS に依存しない）。スケルトン変換、入力パーサー、スケルトン索引辞書、段1（ラティスと N-best） |
| `aiokey-cli` | 変換を試す・測る CLI（`convert`、`bench`） |
| `aiokey-dictbuild` | Mozc の辞書からスケルトン索引辞書（`aiokey.dic`）を作る |
| `aiokey-py` | aiokey-engine を Python から呼ぶ拡張モジュール（`python/` の `aiokey` パッケージ） |
| `aiokey-tsf` | Windows の TSF テキストサービス（`aiokey.dll`） |
| `aiokey-server` | 変換サーバー。Windows は名前付きパイプ、unix は `$XDG_RUNTIME_DIR/aiokey_server.sock` で待ち受ける（`XDG_RUNTIME_DIR` がないと起動しない） |
| `aiokey-ui` | 候補ウィンドウとモード表示 |
| `aiokey-launcher` | サーバーと UI を起動する |
| `aiokey-ipc` | gRPC の定義と設定ファイル |
| `aiokey-macros` | TSF 実装用のマクロ |

## 開発

開発は Docker の dev コンテナで行います。Windows では、WSL の中の Docker Engine から、リポジトリのルート（`/mnt/c/...`）で実行します（例：`wsl -d Ubuntu --cd /mnt/c/<リポジトリのパス> -- docker compose run --rm dev cargo test-portable`）。

```sh
# イメージを作る
docker compose build dev

# Mozc の辞書データを取得する（data/mozc。テストと辞書づくりに使う）
scripts/fetch-mozc.sh

# スケルトン索引辞書を作る
docker compose run --rm dev cargo run --release -p aiokey-dictbuild   # build/aiokey.dic に出力

# 変換を試す
docker compose run --rm dev cargo run --release -p aiokey-cli -- convert htsthqtun

# OS に依存しないクレートのテスト
docker compose run --rm dev cargo test-portable

# 学習・評価データを作る（Wikipedia の記事を取り出し、例を作る）
docker compose run --rm dev sh -c "cd python && uv run python -m aiokey.corpus --limit 300 --out ../data/wiki/articles.jsonl"
docker compose run --rm dev sh -c "cd python && uv run python -m aiokey.datagen --articles ../data/wiki/articles.jsonl --dict ../build/aiokey.dic --out ../data/examples"

# 段1の精度を測る（test セットで再現率・top-1・top-5・打鍵削減率など）
docker compose run --rm dev sh -c "cd python && uv run python -m aiokey eval --examples ../data/examples/test.jsonl --dict ../build/aiokey.dic"

# Python パッケージ（データ生成・評価用）のテスト
docker compose run --rm dev sh -c "cd python && uv run pytest"

# Windows 向けバイナリのクロスビルド（build/ に出力）
docker compose run --rm dev scripts/build-windows.sh
```

正式なリリースビルドは、`v*` タグを push したときに GitHub Actions の Windows ランナーで行います。

## クレジット

AI-O Key は [azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows)（MIT）をフォークして作っています。辞書データには [Mozc](https://github.com/google/mozc)（BSD-3-Clause）を、採点モデルには [zenz-v2.5-xsmall](https://huggingface.co/Miwa-Keita/zenz-v2.5-xsmall)（CC-BY-SA 4.0）を使います。詳しくは [NOTICE](NOTICE) を見てください。

## ライセンス

ソースコードは MIT License です（[LICENSE](LICENSE)）。学習済みモデルは CC-BY-SA 4.0 を継承します。
