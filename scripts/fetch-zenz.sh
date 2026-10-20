#!/usr/bin/env bash
# zenz-v2.5-xsmall を固定リビジョンで取得して SHA-256 を確かめ、GGUF（Q8_0）に変換する。
#
#   scripts/fetch-zenz.sh                  取得・検証・変換（dev コンテナの中で実行する）
#   scripts/fetch-zenz.sh --verify-only    取得も変換もせず、置いてあるファイルだけを検証する
#   オプション: --dir DIR（モデルの置き場所）、--sums FILE（ハッシュ一覧）、--out FILE（GGUF の出力先）
#
# zenz は CC-BY-SA 4.0（NOTICE を参照）。変換には、推論に使う llama.cpp（llama-cpp-sys-2 0.1.158）と
# 同じコミットの convert_hf_to_gguf.py を使う。
set -euo pipefail

ZENZ_REPO="Miwa-Keita/zenz-v2.5-xsmall"
ZENZ_REVISION="9bfb00e795f89284fe9164f0fbde171d046fc546"
LLAMA_CPP_COMMIT="26394b4e6749a41c3633db040e0987500a5f7013"
BASE_URL="https://huggingface.co/${ZENZ_REPO}/resolve/${ZENZ_REVISION}"

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/data/zenz-v2.5-xsmall"
sums="$root/scripts/zenz.sha256"
out="$root/data/models/zenz-v2.5-xsmall-q8_0.gguf"
verify_only=0

while [ $# -gt 0 ]; do
  case "$1" in
    --verify-only) verify_only=1 ;;
    --dir|--sums|--out)
      [ $# -ge 2 ] || { echo "$1 needs a value" >&2; exit 2; }
      case "$1" in --dir) dir="$2" ;; --sums) sums="$2" ;; --out) out="$2" ;; esac
      shift ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

sums="$(cd "$(dirname "$sums")" && pwd)/$(basename "$sums")"
mkdir -p "$dir"

if [ "$verify_only" -eq 0 ]; then
  while read -r _hash name; do
    [ -n "$name" ] || continue
    if [ -f "$dir/$name" ] && (cd "$dir" && grep -F " $name" "$sums" | sha256sum -c --status); then
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
echo "verified: $(grep -c . "$sums") files in $dir ($ZENZ_REPO@${ZENZ_REVISION:0:12})"
[ "$verify_only" -eq 1 ] && exit 0

llama="$root/data/llama.cpp-$LLAMA_CPP_COMMIT"
if [ ! -f "$llama/convert_hf_to_gguf.py" ]; then
  echo "fetch llama.cpp converter ($LLAMA_CPP_COMMIT)"
  mkdir -p "$llama"
  curl -fsSL "https://codeload.github.com/ggml-org/llama.cpp/tar.gz/$LLAMA_CPP_COMMIT" \
    | tar -xz -C "$llama" --strip-components=1 --wildcards \
        "*/convert_hf_to_gguf.py" "*/gguf-py/*" "*/conversion/*" "*/requirements/*"
fi
mkdir -p "$(dirname "$out")"
uv run --no-project --python 3.12 --quiet \
  --index https://download.pytorch.org/whl/cpu --index-strategy unsafe-best-match \
  --with-requirements "$llama/requirements/requirements-convert_hf_to_gguf.txt" \
  python "$root/scripts/convert_zenz.py" "$llama" "$dir" "$out"
echo "wrote $out ($(sha256sum "$out" | cut -c1-16)…)"
