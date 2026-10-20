# AI-O Key 段2の学習用コンテナ（CUDA）。GPU サーバーで、dev コンテナと同じ `python -m aiokey train` を動かす。
# GPU があれば CUDA で、なければ CPU で学習する（--device auto）。
#
#   docker build -f docker/train.Dockerfile -t aiokey-train .
#   docker run --rm --gpus all -v "$PWD/data:/work/data" -v "$PWD/build:/work/build" aiokey-train \
#     train --train data/examples/train.jsonl --dev data/examples/dev.jsonl --out build/stage2
#
# 元のモデルと llama.cpp の変換スクリプト（data/）は scripts/fetch-zenz.sh で用意し、ボリュームで渡す。
# CUDA のライブラリは torch（cu128）の wheel に入っているので、土台は CUDA の base イメージでよい。
FROM nvidia/cuda:12.8.1-base-ubuntu24.04

# rust-toolchain.toml の channel と同じ版にする
ARG RUST_VERSION=1.98.0
# イメージ作成中の cargo の並列数
ARG BUILD_JOBS=4

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_BUILD_JOBS=${BUILD_JOBS} \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    UV_PROJECT_ENVIRONMENT=/opt/venv \
    UV_LINK_MODE=copy \
    PATH=/opt/venv/bin:/opt/cargo/bin:$PATH

# 拡張モジュール（aiokey._engine）は llama.cpp を含むので、C++ のビルドと bindgen 用の libclang が要る
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl build-essential cmake pkg-config clang libclang-dev \
        python3 python3-venv \
    && rm -rf /var/lib/apt/lists/*

RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION}

COPY --from=ghcr.io/astral-sh/uv:0.12.5 /uv /uvx /usr/local/bin/

WORKDIR /work
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
COPY python python
COPY scripts scripts
RUN cd python && uv sync --frozen --no-dev --extra cuda --python /usr/bin/python3.12 \
    && rm -rf /opt/cargo/registry target

ENTRYPOINT ["python", "-m", "aiokey"]
CMD ["train", "--help"]
