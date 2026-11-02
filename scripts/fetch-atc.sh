#!/usr/bin/env bash
# ATC（Alternative Typing Contest、https://o24.works/atc/）の 2026 年のお題を取得し、SHA-256 を確かめる（#39 の評価用）。
#
#   scripts/fetch-atc.sh                  取得して検証する（既定の置き場所は data/atc）
#   scripts/fetch-atc.sh --verify-only    取得せず、置いてあるファイルだけを検証する
#   オプション: --dir DIR（置き場所）
#
# お題の本文はサイトのもので、転載の許可がないのでリポジトリには入れない。サイトがお題を差し替えると
# ハッシュが合わなくなるので、そのときは中身を確かめてから scripts/atc.sha256 を更新する。
set -euo pipefail

URL="https://o24.works/atc/atc26.txt"

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/data/atc"
sums="$root/scripts/atc.sha256"
verify_only=0

while [ $# -gt 0 ]; do
  case "$1" in
    --verify-only) verify_only=1 ;;
    --dir)
      [ $# -ge 2 ] || { echo "$1 needs a value" >&2; exit 2; }
      dir="$2"
      shift ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

mkdir -p "$dir"
if [ "$verify_only" -eq 0 ]; then
  echo "fetch atc26.txt"
  curl -fsSL --retry 3 -o "$dir/atc26.txt.part" "$URL"
  mv "$dir/atc26.txt.part" "$dir/atc26.txt"
fi

# 一致しなければ、sha256sum が名前を出して失敗する
(cd "$dir" && sha256sum -c --quiet "$sums")
echo "verified: atc26.txt in $dir"
