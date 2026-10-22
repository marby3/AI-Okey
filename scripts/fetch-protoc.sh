#!/usr/bin/env bash
# Windows の GitHub Actions（Git Bash）で、版を固定した protoc を取得して SHA-256 を確かめ、PATH に足す。
#   bash scripts/fetch-protoc.sh
#
# arduino/setup-protoc は、版を指定しないと 23.x をリリースの一覧から探し、見つからなくなって失敗した（#37）。
# 版を上げるときは、PROTOC_VERSION と PROTOC_SHA256（GitHub のリリースのアセットの digest と同じ値）を両方変える。
set -euo pipefail

PROTOC_VERSION="36.2"
PROTOC_SHA256="f0c128dc0d8492eceece83bb459a4c0e316764b929ffbf1aa416357fd644edd3"

name="protoc-${PROTOC_VERSION}-win64.zip"
dir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/protoc-${PROTOC_VERSION}"
mkdir -p "$dir"
curl -fsSL --retry 3 -o "$dir/$name" \
  "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/${name}"
(cd "$dir" && echo "${PROTOC_SHA256}  ${name}" | sha256sum -c --quiet -)
unzip -q -o "$dir/$name" -d "$dir"
"$dir/bin/protoc.exe" --version

# 後の手順で使えるようにする（prost-build は PROTOC を見る）
if [ -n "${GITHUB_PATH:-}" ]; then
  cygpath -w "$dir/bin" >> "$GITHUB_PATH"
  echo "PROTOC=$(cygpath -w "$dir/bin/protoc.exe")" >> "$GITHUB_ENV"
fi
