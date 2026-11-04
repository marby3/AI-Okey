#!/usr/bin/env bash
# 段1の並べ替えに使う文字 n-gram の表（aiokey-ngram.bin）を、表専用の Release から取得して SHA-256 を確かめる。
#   bash scripts/fetch-ngram.sh [DIR]               DIR（既定は .）に取得・検証する（gh の認証が要る）
#   bash scripts/fetch-ngram.sh --verify-only DIR   取得せず、DIR に置いてあるファイルだけを確かめる
#
# 表はコーパスから `python -m aiokey ngram` で作り、リポジトリには入れない（#58。docs/training-data.md）。
# 作り直したら、新しい TABLE_TAG の Release に置き、ここの 2 つの値を変える。
set -euo pipefail

TABLE_TAG="stage1-ngram-v1"
TABLE_SHA256="e220eeee304eebe065c11738780d1cf8347b1cac0572bd1dd6e102e4119a42ce"

# 一致しなければ、sha256sum がファイル名を出して失敗する
verify() {
  (cd "$1" && printf '%s  %s\n' "$TABLE_SHA256" aiokey-ngram.bin | sha256sum -c --quiet -)
}

if [ "${1:-}" = "--verify-only" ]; then
  [ $# -eq 2 ] || { echo "usage: $0 --verify-only DIR" >&2; exit 2; }
  verify "$2"
  exit 0
fi

dir="${1:-.}"
mkdir -p "$dir"
gh release download "$TABLE_TAG" --repo marby3/AI-Okey --dir "$dir" --clobber --pattern aiokey-ngram.bin
verify "$dir"
echo "fetched $TABLE_TAG into $dir"
