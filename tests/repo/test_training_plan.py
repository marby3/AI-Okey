"""Issue #41 の受け入れテスト（基準 6・7）：GPU サーバーでそのまま始められる学習の手順と、学習の Issue。"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_gpu_procedure_grows_the_data_step_by_step_and_says_when_to_stop():
    # 基準 6：新しいデータの送り方、段階的に増やす手順と終える条件、所要時間の見積もり、CPU で流した記録
    doc = (ROOT / "docs/training-gpu.md").read_text(encoding="utf-8")
    for item in ["--eval-every", "--patience", "--limit", "段階", "終える条件", "所要時間", "train.jsonl"]:
        assert item in doc, item
    assert re.search(r"\d+(\.\d+)?\s*時間", doc)
    data = (ROOT / "docs/training-data.md").read_text(encoding="utf-8")
    start = data.index("## CPU で流した確認")
    s = data[start : data.find("\n## ", start + 1)]
    for item in ["GGUF", "温度", "止ま"]:
        assert item in s, item


def test_gpu_training_is_split_into_its_own_issue():
    # 基準 7：GPU で学習し直して比べる作業は #55
    doc = (ROOT / "docs/training-gpu.md").read_text(encoding="utf-8")
    assert "#55" in doc
