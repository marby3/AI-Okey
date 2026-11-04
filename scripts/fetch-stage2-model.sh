#!/usr/bin/env bash
# 学習済みの段2（GGUF）と校正値（aiokey-stage2.json）を、モデル専用の Release から取得して SHA-256 を確かめる。
#   bash scripts/fetch-stage2-model.sh [DIR]               DIR（既定は .）に取得・検証する（gh の認証が要る）
#   bash scripts/fetch-stage2-model.sh --verify-only DIR   取得せず、DIR に置いてあるファイルだけを確かめる
#
# モデルは zenz-v2.5-xsmall（CC-BY-SA 4.0）を学習し直したもので、リポジトリには入れない（#13）。
# 学習し直したら、新しい MODEL_TAG の Release に置き、ここの 3 つの値を変える。
set -euo pipefail

MODEL_TAG="stage2-model-v2"
GGUF_SHA256="a77dea520921550b25aaec9a4a9a575796848d412fd0f2d2133e71c70a1c6748"
INFO_SHA256="3c2311024539d20532454768caa9815debe13e4910f053b755898b28804a0601"

# 一致しなければ、sha256sum がファイル名を出して失敗する
verify() {
  (cd "$1" && printf '%s  %s\n%s  %s\n' \
    "$GGUF_SHA256" aiokey-stage2-q8_0.gguf "$INFO_SHA256" aiokey-stage2.json | sha256sum -c --quiet -)
}

if [ "${1:-}" = "--verify-only" ]; then
  [ $# -eq 2 ] || { echo "usage: $0 --verify-only DIR" >&2; exit 2; }
  verify "$2"
  exit 0
fi

dir="${1:-.}"
mkdir -p "$dir"
gh release download "$MODEL_TAG" --repo marby3/AI-Okey --dir "$dir" --clobber \
  --pattern aiokey-stage2-q8_0.gguf --pattern aiokey-stage2.json
verify "$dir"
echo "fetched $MODEL_TAG into $dir"
