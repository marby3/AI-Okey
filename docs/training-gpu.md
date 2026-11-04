# GPU サーバーで段2を学習する

レンタルの GPU サーバー（`ssh gpu`、Ubuntu、x86_64、RTX A4000 16GB）で `python -m aiokey train` を動かす手順。
サーバーは学習のあいだだけ起動してもらうので、準備は手元で済ませ、サーバーでは学習と回収だけをする。

#41 で学習データを大きく増やした（`docs/training-data.md`）。**データの量を段階的に増やして dev の伸びを見て、伸びが
止まったら終える**（下の「4. 学習する」）。1 回の学習の中でも、dev が良くならなくなったら止め、いちばんよかった時点を
書き出す（過学習を防ぐ）。この手順で学習し直して比べる作業は #55。比べるときは評価の例を作り直さず、今ある `target/eval/*.jsonl` と
`target/examples2000/test.jsonl` を使う（`docs/training-data.md` の「評価に使う文を除く」）。

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
```

学習の例（#41 で作った `target/examples-v2`）は、圧縮したまま送って置く（`python -m aiokey train` は `.gz` をそのまま
読む）。train には段1の候補に正解がある例だけが入っている（`aiokey.datagen --train-in-stage1-only`）。送る前に、
サーバーのディスクの空き（`df -h ~`）が圧縮したファイル（#41 のデータで train 395 MB、dev 112 MB）より 5 GB 以上多いことを確かめる。

```bash
docker compose run --rm -T dev bash -c 'cd /work/target/examples-v2 && gzip -1 -kf train.jsonl dev.jsonl'
docker compose run --rm -T dev tar -C /work/target/examples-v2 -cf - train.jsonl.gz dev.jsonl.gz \
  | $SSH 'mkdir -p ~/aiokey-train/data/examples && tar -xf - -C ~/aiokey-train/data/examples'
```

送るのは文から作った学習の例だけで、コーパスそのもの（`data/corpora`）は送らない。学習が終わったら消す（「5. 回収して片付ける」）。

## 3. CUDA 版の torch と拡張モジュールを入れる（サーバー）

```bash
. ~/aiokey-train/env.sh
cd $T/repo/python && uv sync --extra cuda --python 3.12
$T/venv/bin/python -c "import torch, aiokey; print(torch.cuda.get_device_name(0))"
```

## 4. 学習する（サーバー）

### メモリを確かめる

学習の例は読み込んでメモリに持つ。1 例あたり約 5.8 KB（候補 32 件、#41 で測った値）なので、100 万例で約 6 GB になる。
`free -g` の空きに収まる段階まで進める（収まらない段階は飛ばす）。

### まず小さく流す

本番の前に、`--limit` で train・dev とも 200 例に絞り、`--stop-after 50` で 50 ステップだけ流して、動くことと
1 ステップの時間を確かめる（GPU のメモリが足りなければ `--max-batch-tokens` を下げる）。

```bash
. ~/aiokey-train/env.sh
cd $T/repo/python
$T/venv/bin/python -m aiokey train --train $T/data/examples/train.jsonl.gz --dev $T/data/examples/dev.jsonl.gz \
  --limit 200 --out $T/out-try --checkpoint $T/checkpoint-try --epochs 4 --max-batch-tokens 32768 --lr 2e-4 --stop-after 50
```

### データの量を段階的に増やす

`--train-sample` で train から無作為に選ぶ例の数を、倍ずつ増やして学習する。dev はどの段階でも同じ例
（`--dev-sample 20000`、同じ `--seed`）で測るので、段階どうしを比べられる。

- `--eval-every 8000 --patience 3`：8,000 ステップ（約 15 万例）ごとに dev を測り、3 回続けて良くならなければ止める。
  いちばんよかった時点を書き出す（エポックの終わりにも測る）。dev 20,000 例を 1 回測るのに 7〜8 分かかるので、
  測る時間が学習の 2 割を超えない間隔にした。小さい段階（2 万・4 万例）は 4 エポックでも 8,000 ステップに届かないので、
  エポックの終わりにだけ測る（そのため、ほぼ 4 エポック流れる）。
- `--train-sample` は、train を `--seed` で混ぜた並びの先頭から取る。小さい段階の例は大きい段階の例に含まれる。
- チェックポイントは、いちばんよかった時点のモデルの写しも持つので、段階ごとに約 0.4 GB になる（ディスクに注意）。
- `--epochs 4`：上限。大きい段階は、たいてい途中で止まる。
- 途中でサーバーが止まったら、同じ引数に `--resume` を足して続きから流す（`--checkpoint` を段階ごとに分けておく）。

```bash
. ~/aiokey-train/env.sh
cd $T/repo/python
export T E=$T/data/examples
mkdir -p $T/logs
nohup bash -c '
for n in 20000 40000 80000 160000 320000 640000 1280000 1401909; do
  $T/venv/bin/python -m aiokey train --train $E/train.jsonl.gz --dev $E/dev.jsonl.gz \
    --train-sample $n --dev-sample 20000 --out $T/out/$n --checkpoint $T/checkpoint/$n \
    --epochs 4 --max-batch-tokens 32768 --lr 2e-4 --eval-every 8000 --patience 3 > $T/logs/$n.log 2>&1 || exit 1
  # 終える条件：前の段階より dev の top-1（段1の候補に正解がある例）が 0.5 ポイント未満しか上がらなければ終える
  $T/venv/bin/python - "$T/out" <<"PY" || break
import json, pathlib, sys
runs = sorted((int(p.name), json.loads((p / "train.json").read_text())["dev"].get("top1_in_stage1") or 0.0)
              for p in pathlib.Path(sys.argv[1]).iterdir() if (p / "train.json").is_file())
print(runs, file=sys.stderr)
sys.exit(1 if len(runs) >= 2 and runs[-1][1] - runs[-2][1] < 0.005 else 0)
PY
done' > $T/train.log 2>&1 &
```

進み具合は `tail -f $T/train.log $T/logs/*.log` で見る。ssh が切れても止まらないよう `nohup` で流す。

### 終える条件

- **段階の間**：データを倍にしても、dev の top-1（段1の候補に正解がある例）が **0.5 ポイント未満**しか上がらなければ
  （下がったときも）、そこで終える。使うのは、dev がよかったほうの段階のモデル。
- **段階の中**：dev の decision loss が `--patience` 回続けて良くならなければ止め、いちばんよかった時点を使う。
- 0.5 ポイントは案。dev 20,000 例のうち段1の候補に正解がある例は約 6,000 で、同じ例で比べても 0.5 ポイントの差は
  ゆれと見分けにくい。#55 で、伸び方を見て決め直す。

### 所要時間の見積もり

GPU サーバー（RTX A4000、`--max-batch-tokens 32768`）で、#41 の新しいデータから無作為に選んだ 5,000 例を流して測った
（2026-11-03）：

| 項目 | 測った値 |
|---|---|
| 学習 | 1 秒あたり 57 例（5,000 例 1 エポックが 88 秒、272 ステップ。1 ステップ約 18 例） |
| dev を測る | 2,000 例で約 45 秒（20,000 例なら 7〜8 分） |
| GPU のメモリ | 最大 9.96 GB |

これをもとにした、各段階の時間（dev を測る時間は含まない）：

| 段階（`--train-sample`） | 1 エポック | 4 エポック（上限） |
|---|---|---|
| 20,000 | 6 分 | 23 分 |
| 40,000 | 12 分 | 47 分 |
| 80,000 | 23 分 | 1.6 時間 |
| 160,000 | 47 分 | 3.1 時間 |
| 320,000 | 1.6 時間 | 6.2 時間 |
| 640,000 | 3.1 時間 | 12.5 時間 |
| 1,280,000 | 6.2 時間 | 25.0 時間 |
| 1,401,909 | 6.8 時間 | 27.3 時間 |

全部の段階を 1 エポックずつ流すと約 19 時間、上限の 4 エポックずつなら約 77 時間。各段階の始めに、
train（gzip で 395 MB、約 140 万例）を 2 回読んで例を選ぶので、数分ずつ足される（1 回目は行の文字列を見るだけ）。

大きい段階は 1〜2 エポックで止まる見込みで、終える条件で途中の段階で終わることも多い。時間の制限は設けない（2026-11-03、ユーザー）。

## 5. 回収して片付ける

```bash
$SSH 'tar -C ~/aiokey-train/out -cf - .' | tar -xf - -C build/stage2
$SSH 'tar -C ~/aiokey-train -cf - logs train.log' | tar -xf - -C build/stage2
```

`build/stage2` に、`aiokey-stage2-q8_0.gguf`（段2のモデル）、`train.json`（温度、損失の記録）、`hf/`（Hugging Face 形式）が入る。
段階ごとに学習したときは、`out/<例の数>/` ごとに入る。採用する段階を決めたら、その段階のフォルダを回収する。

回収したら、学習の例をサーバーから消す（`rm -rf ~/aiokey-train/data/examples ~/aiokey-train/checkpoint`）。そのあと、
サーバーの停止をユーザーに頼む。`~/aiokey-train` の道具は、次の学習で使うので残してよい（消すときはフォルダごと消す）。

## 6. データを増やして続きから学習する

#55 は 160,000 例で止めた（`docs/eval/stage2-v3.md`）。増やすときは、そのモデル（手元の `data/stage2-v3/out/160000/hf`。回収してある）を
サーバーに送り、`--base` に渡して続きから学習する。学習の例はサーバーから消してあるので、手元の `target/examples-v2` から送り直す（「2. コードとデータを送る」）。

- **今の `--train-sample` だけでは重複する。** 混ぜた並びの先頭から N 例を取るので、`--train-sample 320000` には学習済みの 160,000 例がそのまま入る。
  重複を避けるには、並びの先頭の使った分を飛ばすオプション（`--train-skip` など）を足す（未実装）。
- 飛ばせるのは「例」の単位。1 つの文から複数の例（子音だけ・母音あり・途中で切ったもの）を作るので、同じ文の別の例は残る。
- どの例を使ったかの一覧は残していない。同じファイル、同じ `--seed`（0）なら同じ並びになるはずだが、サーバーと手元の Python で並びが同じかは
  学習の前に確かめる。
- `--resume` は使えない（チェックポイントは消した。160,000 例は最後まで流れて学習率も 0 まで下がっていたので、再開する途中の状態もない）。
  `--base` では学習率がウォームアップからやり直しになるので、`2e-4` のままでよいかは dev の伸びを見て決める。
