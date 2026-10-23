# GPU サーバーで段2を学習する

レンタルの GPU サーバー（`ssh gpu`、Ubuntu、x86_64、RTX A4000 16GB）で `python -m aiokey train` を動かす手順。
サーバーは学習のあいだだけ起動してもらうので、準備は手元で済ませ、サーバーでは学習と回収だけをする。

サーバーのディスクは空きが少ない（2026-10 時点で約 17GB）ので、Docker のイメージ（`docker/train.Dockerfile`、
数十 GB）は使わず、uv と Rust を `~/aiokey-train` の下に入れる。システムには手を入れないので、片付けはこのフォルダを消すだけでよい。

## 1. 道具を入れる（初回だけ）

```bash
T=$HOME/aiokey-train
mkdir -p $T/bin $T/repo $T/data
export CARGO_HOME=$T/cargo RUSTUP_HOME=$T/rustup PATH=$T/bin:$T/cargo/bin:$PATH
curl -fsSL https://github.com/astral-sh/uv/releases/download/0.12.5/uv-x86_64-unknown-linux-gnu.tar.gz \
  | tar -xz -C $T/bin --strip-components=1
curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.98.0 --no-modify-path
# 拡張モジュール（llama.cpp の bindgen）が使う libclang は、Python の wheel から取る
uv venv -q --python 3.12 $T/libclang && VIRTUAL_ENV=$T/libclang uv pip install -q libclang==18.1.1
```

環境変数は `$T/env.sh` にまとめておく。pip の libclang には clang の標準ヘッダーがないので、gcc のものを渡す。

```bash
cat > $T/env.sh <<'EOS'
T=$HOME/aiokey-train
export CARGO_HOME=$T/cargo RUSTUP_HOME=$T/rustup PATH=$T/bin:$T/cargo/bin:$PATH
export LIBCLANG_PATH=$T/libclang/lib/python3.12/site-packages/clang/native
export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/x86_64-linux-gnu/13/include"
export UV_PROJECT_ENVIRONMENT=$T/venv CARGO_BUILD_JOBS=12 CARGO_TARGET_DIR=$T/target
# バッチの大きさが変わっても GPU のメモリが細切れにならないように
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
EOS
```

## 2. コードとデータを送る（手元から）

サーバーには rsync がないので、送るのは tar を ssh に流す（`ssh gpu` は ~/.ssh/config のポート転送が
ほかの接続と重なると警告を出すので、`-o ClearAllForwardings=yes` を付ける）。サーバーに rsync があれば
`rsync -a` でもよい。

```bash
SSH="ssh -o ClearAllForwardings=yes gpu"
git archive HEAD Cargo.toml Cargo.lock rust-toolchain.toml crates python scripts | $SSH 'tar -xf - -C ~/aiokey-train/repo'
tar -C data -cf - zenz-v2.5-xsmall llama.cpp-<commit> | $SSH 'mkdir -p ~/aiokey-train/repo/data && tar -xf - -C ~/aiokey-train/repo/data'
docker compose run --rm -T dev tar -C /work/target/examples -cf - train.jsonl dev.jsonl test.jsonl \
  | $SSH 'mkdir -p ~/aiokey-train/data/examples && tar -xf - -C ~/aiokey-train/data/examples'
```

## 3. CUDA 版の torch と拡張モジュールを入れる（サーバー）

```bash
. ~/aiokey-train/env.sh
cd $T/repo/python && uv sync --extra cuda --python 3.12
$T/venv/bin/python -c "import torch, aiokey; print(torch.cuda.get_device_name(0))"
```

## 4. 学習する（サーバー）

途中で止まっても続きから再開できるよう、`--checkpoint` を必ず付ける。まず `--stop-after` で数十ステップだけ
流して 1 ステップの時間を測り、残りの時間を見積もってから続きを `--resume` で流す。

```bash
. ~/aiokey-train/env.sh
cd $T/repo/python
$T/venv/bin/python -m aiokey train \
  --train $T/data/examples/train.jsonl --dev $T/data/examples/dev.jsonl \
  --out $T/out --checkpoint $T/checkpoint --epochs 4 --max-batch-tokens 32768 --lr 2e-4 --stop-after 50
# 続き（同じ引数に --resume を足す。--stop-after は外す）
nohup $T/venv/bin/python -m aiokey train \
  --train $T/data/examples/train.jsonl --dev $T/data/examples/dev.jsonl \
  --out $T/out --checkpoint $T/checkpoint --epochs 4 --max-batch-tokens 32768 --lr 2e-4 --resume > $T/train.log 2>&1 &
```

ssh が切れても学習が止まらないよう、本番は `nohup` で流す。進み具合は `tail -f $T/train.log` で見る。

## 5. 回収して片付ける

```bash
$SSH 'tar -C ~/aiokey-train/out -cf - .' | tar -xf - -C build/stage2
```

`build/stage2` に、`aiokey-stage2-q8_0.gguf`（段2のモデル）、`train.json`（温度、損失の記録）、`hf/`（Hugging Face 形式）が入る。
回収したら、サーバーの停止をユーザーに頼む。`~/aiokey-train` は、次の学習で使うので残してよい（消すときはフォルダごと消す）。
