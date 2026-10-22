#!/usr/bin/env bash
# Windows の GitHub Actions（Git Bash）で、版を固定した protoc を取得して SHA-256 を確かめ、PATH に足す。
#   bash scripts/fetch-protoc.sh                     取得・検証して PATH に足す
#   bash scripts/fetch-protoc.sh --verify-only ZIP   取得せず、置いてある zip の SHA-256 だけを確かめる
#
# arduino/setup-protoc は、版を指定しないと 23.x をリリースの一覧から探し、見つからなくなって失敗した（#37）。
# 版を上げるときは、PROTOC_VERSION と PROTOC_SHA256（GitHub のリリースのアセットの digest と同じ値）を両方変える。
set -euo pipefail

PROTOC_VERSION="36.2"
PROTOC_SHA256="f0c128dc0d8492eceece83bb459a4c0e316764b929ffbf1aa416357fd644edd3"

# 一致しなければ、sha256sum がファイル名を出して失敗する
verify() {
  (cd "$(dirname "$1")" && echo "${PROTOC_SHA256}  $(basename "$1")" | sha256sum -c --quiet -)
}

if [ "${1:-}" = "--verify-only" ]; then
  [ $# -eq 2 ] || { echo "usage: $0 --verify-only ZIP" >&2; exit 2; }
  verify "$2"
  exit 0
fi

zip_name="protoc-${PROTOC_VERSION}-win64.zip"
install_dir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/protoc-${PROTOC_VERSION}"
mkdir -p "$install_dir"
curl -fsSL --retry 3 -o "$install_dir/$zip_name" \
  "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/${zip_name}"
verify "$install_dir/$zip_name"
unzip -q -o "$install_dir/$zip_name" -d "$install_dir"
"$install_dir/bin/protoc.exe" --version

# 後の手順で使えるようにする（prost-build は PROTOC を見る）
if [ -n "${GITHUB_PATH:-}" ]; then
  cygpath -w "$install_dir/bin" >> "$GITHUB_PATH"
  echo "PROTOC=$(cygpath -w "$install_dir/bin/protoc.exe")" >> "$GITHUB_ENV"
fi
