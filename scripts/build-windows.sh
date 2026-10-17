#!/usr/bin/env bash
# Windows 向けバイナリを cargo-xwin でクロスビルドし、build/ に集める。
# dev コンテナの中で実行する: docker compose -f docker/compose.yml run --rm dev scripts/build-windows.sh [release|debug]
set -euo pipefail

profile="${1:-release}"
flags=()
[ "$profile" = "release" ] && flags+=(--release)

cargo xwin build "${flags[@]}" --target x86_64-pc-windows-msvc \
  -p aiokey-tsf -p aiokey-server -p aiokey-ui -p aiokey-launcher
cargo xwin build "${flags[@]}" --target i686-pc-windows-msvc -p aiokey-tsf

out=build
rm -rf "$out"
mkdir -p "$out/x86"
x64="target/x86_64-pc-windows-msvc/$profile"
cp "$x64/aiokey.dll" "$x64/aiokey-server.exe" "$x64/aiokey-ui.exe" "$x64/aiokey-launcher.exe" "$out/"
cp "target/i686-pc-windows-msvc/$profile/aiokey.dll" "$out/x86/"
ls -l "$out" "$out/x86"
