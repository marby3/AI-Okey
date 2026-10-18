"""Issue #2 の受け入れテスト：Python からスケルトン変換器を呼べる。"""

import aiokey


def test_skeletonize_from_python():
    assert aiokey.skeletonize("はたしてほんとうに") == "htsthqtun"
