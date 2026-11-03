#!/usr/bin/env bash
# 段2の学習に加えるコーパスを取得し、SHA-256 を確かめる（#41）。どれを使うか、ライセンス、Web のコーパスを使う
# 法的な根拠は docs/training-data.md。Wikipedia と Wikinews はダンプから直接読む（python -m aiokey.corpus）。
#
#   scripts/fetch-corpora.sh                  すべて取得して検証する（置き場所は data/corpora）
#   scripts/fetch-corpora.sh tatoeba aozora   名前を挙げたものだけ
#   scripts/fetch-corpora.sh --verify-only    取得せず、置いてあるファイルだけを検証する
#   オプション: --dir DIR（置き場所）
#
# 取得したデータはリポジトリにも配布物にも入れない（data/ は .gitignore で除いている）。
# Tatoeba は毎週土曜に書き出しを差し替え、古い版を残さない。ハッシュが合わなくなったら、中身を確かめてから
# scripts/corpora.sha256 と、docs/training-data.md の取得日を更新する。ほかは版（コミット）を固定している。
set -euo pipefail

COMMON_VOICE_COMMIT="1e772a6064fa1433b890d111ed8d11eb5cd428e6"
AOZORA_REVISION="42a9c9c0f1d67e6a5554d9bea4201973dc9b049c"
FINEWEB2_REVISION="af9c13333eb981300149d5ca60a8e9d659b276b9"

# 名前  置き場所（data/corpora の下）  URL
FILES=(
  "tatoeba tatoeba/jpn_sentences.tsv.bz2 https://downloads.tatoeba.org/exports/per_language/jpn/jpn_sentences.tsv.bz2"
  "common-voice common-voice/sentence-collector.txt https://raw.githubusercontent.com/common-voice/common-voice/$COMMON_VOICE_COMMIT/server/data/ja/sentence-collector.txt"
  "common-voice common-voice/yumie-text-1.txt https://raw.githubusercontent.com/common-voice/common-voice/$COMMON_VOICE_COMMIT/server/data/ja/yumie-text-1.txt"
  "aozora aozora/aozorabunko-dedupe-clean.jsonl.gz https://huggingface.co/datasets/globis-university/aozorabunko-clean/resolve/$AOZORA_REVISION/aozorabunko-dedupe-clean.jsonl.gz"
  "web fineweb-2/000_00000.parquet https://huggingface.co/datasets/HuggingFaceFW/fineweb-2/resolve/$FINEWEB2_REVISION/data/jpn_Jpan/train/000_00000.parquet"
)

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/data/corpora"
sums="$root/scripts/corpora.sha256"
verify_only=0
wanted=()

while [ $# -gt 0 ]; do
  case "$1" in
    --verify-only) verify_only=1 ;;
    --dir)
      [ $# -ge 2 ] || { echo "$1 needs a value" >&2; exit 2; }
      dir="$2"
      shift ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) wanted+=("$1") ;;
  esac
  shift
done

selected() {
  [ ${#wanted[@]} -eq 0 ] && return 0
  local w
  for w in "${wanted[@]}"; do [ "$w" = "$1" ] && return 0; done
  return 1
}

status=0
for entry in "${FILES[@]}"; do
  read -r name path url <<<"$entry"
  selected "$name" || continue
  expected="$(awk -v p="$path" '$2 == p { print $1 }' "$sums")"
  [ -n "$expected" ] || { echo "$path: no hash in $sums" >&2; exit 1; }
  mkdir -p "$(dirname "$dir/$path")"
  if [ "$verify_only" -eq 0 ]; then
    echo "fetch $path"
    curl -fSL --retry 3 -o "$dir/$path.part" "$url"
    actual="$(sha256sum "$dir/$path.part" | cut -d' ' -f1)"
    if [ "$actual" != "$expected" ]; then
      # 一致しなければ使わずに消す
      rm -f "$dir/$path.part"
      echo "$path: SHA-256 mismatch (expected $expected, got $actual)" >&2
      status=1
      continue
    fi
    mv "$dir/$path.part" "$dir/$path"
  fi
  if [ -f "$dir/$path" ] && [ "$(sha256sum "$dir/$path" | cut -d' ' -f1)" = "$expected" ]; then
    echo "ok $path"
  else
    echo "$path: missing or SHA-256 mismatch" >&2
    status=1
  fi
done
exit $status
