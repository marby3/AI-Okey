"""zenz（Hugging Face 形式）を GGUF に変換する。scripts/fetch-zenz.sh から呼ぶ。

    python scripts/convert_zenz.py <llama.cpp のフォルダ> <zenz のフォルダ> <出力 .gguf> [q8_0|f32]

llama.cpp の convert_hf_to_gguf.py は、トークナイザーの前処理の種類を既知のハッシュで見分ける。
zenz は GPT-2 と同じ GPT2Tokenizer（vocab.json と merges.txt）を使うが、ハッシュが登録されていないので、
見分ける関数だけを "gpt-2" を返すものに差し替えてから変換する。

また、zenz の config.json は eos_token_id が 2 だが、語彙（vocab.json）では 2 が <s>、3 が </s> になっている
（モデルは </s> で文を終える）。変換器は config.json の値を使うので、特殊トークンの ID を
special_tokens_map.json と語彙から付け直す（#26）。
"""

import json
import sys
from pathlib import Path

llama_cpp, model_dir, out = sys.argv[1:4]
outtype = sys.argv[4] if len(sys.argv) > 4 else "q8_0"
sys.path.insert(0, llama_cpp)
sys.path.insert(1, str(Path(llama_cpp) / "gguf-py"))

import conversion.base  # noqa: E402
import gguf  # noqa: E402

conversion.base.TextModel.get_vocab_base_pre = lambda self, tokenizer: "gpt-2"

# 変換器は SpecialVocab を中で作るので、外から ID を渡す口がない。このスクリプトは変換 1 回だけのプロセスなので、
# 読み込みの関数を差し替えて、読んだあとに ID を付け直す
_load = gguf.SpecialVocab._load
SPECIAL = ("bos", "eos", "unk", "pad")


def _load_from_vocab(self, path):
    _load(self, path)
    tokens = json.loads((path / "special_tokens_map.json").read_text(encoding="utf-8"))
    vocab = json.loads((path / "vocab.json").read_text(encoding="utf-8"))
    for kind in SPECIAL:
        token = tokens.get(f"{kind}_token")
        token = token.get("content") if isinstance(token, dict) else token
        if isinstance(token, str) and token in vocab:
            self.special_token_ids[kind] = vocab[token]
        elif kind == "eos":
            # EOS を決められないまま config.json の値で書くと、#26 の誤りに戻るので止める
            sys.exit(f"{path}: eos_token is not in special_tokens_map.json and vocab.json")


gguf.SpecialVocab._load = _load_from_vocab

import convert_hf_to_gguf  # noqa: E402

sys.argv = ["convert_hf_to_gguf.py", model_dir, "--outtype", outtype, "--outfile", out]
convert_hf_to_gguf.main()
