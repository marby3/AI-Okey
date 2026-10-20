"""zenz（Hugging Face 形式）を GGUF に変換する。scripts/fetch-zenz.sh から呼ぶ。

    python scripts/convert_zenz.py <llama.cpp のフォルダ> <zenz のフォルダ> <出力 .gguf>

llama.cpp の convert_hf_to_gguf.py は、トークナイザーの前処理の種類を既知のハッシュで見分ける。
zenz は GPT-2 と同じ GPT2Tokenizer（vocab.json と merges.txt）を使うが、ハッシュが登録されていないので、
見分ける関数だけを "gpt-2" を返すものに差し替えてから変換する。
"""

import sys
from pathlib import Path

llama_cpp, model_dir, out = sys.argv[1:4]
sys.path.insert(0, llama_cpp)
sys.path.insert(1, str(Path(llama_cpp) / "gguf-py"))

import conversion.base  # noqa: E402

conversion.base.TextModel.get_vocab_base_pre = lambda self, tokenizer: "gpt-2"

import convert_hf_to_gguf  # noqa: E402

sys.argv = ["convert_hf_to_gguf.py", model_dir, "--outtype", "q8_0", "--outfile", out]
convert_hf_to_gguf.main()
