#!/usr/bin/env bash
# Mozc のオープンソース辞書（src/data/dictionary_oss）を固定コミットから取得し、SHA-256 を確かめる。
#
#   scripts/fetch-mozc.sh                     取得して検証する（既定の置き場所は data/mozc）
#   scripts/fetch-mozc.sh --verify-only       取得せず、置いてあるファイルだけを検証する
#   オプション: --dir DIR（置き場所）、--sums FILE（ハッシュ一覧。既定は scripts/mozc.sha256）
#
# 辞書のライセンスは取得した README.txt（IPAdic ほか）に従う。NOTICE も参照。
set -euo pipefail

MOZC_COMMIT="0136c35782536cb43d710575c311cd4185982738"
BASE_URL="https://raw.githubusercontent.com/google/mozc/${MOZC_COMMIT}/src/data/dictionary_oss"

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/data/mozc"
sums="$root/scripts/mozc.sha256"
verify_only=0

while [ $# -gt 0 ]; do
  case "$1" in
    --verify-only) verify_only=1 ;;
    --dir|--sums)
      [ $# -ge 2 ] || { echo "$1 needs a value" >&2; exit 2; }
      if [ "$1" = --dir ]; then dir="$2"; else sums="$2"; fi
      shift ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

sums="$(cd "$(dirname "$sums")" && pwd)/$(basename "$sums")"
mkdir -p "$dir"

if [ "$verify_only" -eq 0 ]; then
  while read -r _hash name; do
    [ -n "$name" ] || continue
    if [ -f "$dir/$name" ] && (cd "$dir" && grep " $name\$" "$sums" | sha256sum -c --status); then
      echo "ok (cached) $name"
      continue
    fi
    echo "fetch $name"
    curl -fsSL --retry 3 -o "$dir/$name.part" "$BASE_URL/$name"
    mv "$dir/$name.part" "$dir/$name"
  done < "$sums"
fi

# 1 つでも一致しなければ、sha256sum が名前を出して失敗する
(cd "$dir" && sha256sum -c --quiet "$sums")
echo "verified: $(grep -c . "$sums") files in $dir (mozc ${MOZC_COMMIT:0:12})"
