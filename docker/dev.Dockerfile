# AI-O Key 開発用コンテナ（Linux）
# - OS非依存クレートのテスト、Python のデータ生成・評価
# - cargo-xwin による Windows バイナリ（x86_64 / i686 msvc）のクロスビルド
FROM ubuntu:24.04

# rust-toolchain.toml の channel と同じ版にする
ARG RUST_VERSION=1.98.0
ARG LLVM_VERSION=20
# イメージ作成中の cargo の並列数（PC の負荷を抑えるため。compose.yml の AIOKEY_JOBS から渡す）
ARG BUILD_JOBS=2

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_BUILD_JOBS=${BUILD_JOBS} \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH \
    XWIN_ACCEPT_LICENSE=1 \
    XWIN_ARCH=x86_64,x86 \
    XWIN_CACHE_DIR=/opt/xwin

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl git build-essential pkg-config \
        clang-${LLVM_VERSION} lld-${LLVM_VERSION} llvm-${LLVM_VERSION} \
        cmake ninja-build protobuf-compiler \
        python3 python3-venv \
    && rm -rf /var/lib/apt/lists/* \
    # LLVM のツールは呼ばれた名前で動作を変えるので、パッケージにない名前は実体へのリンクで用意する
    # （clang-cl → clang、lld-link → lld、llvm-lib / llvm-dlltool → llvm-ar）。見つからなければ失敗させる。
    && bin=/usr/lib/llvm-${LLVM_VERSION}/bin \
    && for pair in clang:clang clang++:clang++ clang-cl:clang lld:lld lld-link:lld \
                   llvm-lib:llvm-ar llvm-rc:llvm-rc llvm-ar:llvm-ar llvm-dlltool:llvm-ar; do \
         name=${pair%%:*}; target=${pair#*:}; \
         [ -x "$bin/$target" ] || { echo "missing LLVM tool: $target" >&2; exit 1; }; \
         ln -sf "$bin/$target" /usr/local/bin/$name || exit 1; \
       done \
    && clang-cl --version >/dev/null && lld-link --version >/dev/null && llvm-lib /? >/dev/null

RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION} \
    && rustup component add rustfmt clippy \
    && rustup target add x86_64-pc-windows-msvc i686-pc-windows-msvc \
    && cargo install --locked cargo-xwin --version 0.23.1 \
    && chmod -R a+rwX /opt/cargo /opt/rustup

COPY --from=ghcr.io/astral-sh/uv:0.12.5 /uv /uvx /usr/local/bin/

# MSVC CRT / Windows SDK を事前取得しておく（初回ビルドを速くするため）
RUN cargo new /tmp/warm && cd /tmp/warm \
    && cargo xwin build --target x86_64-pc-windows-msvc \
    && cargo xwin build --target i686-pc-windows-msvc \
    && rm -rf /tmp/warm && chmod -R a+rwX /opt/xwin

WORKDIR /work
