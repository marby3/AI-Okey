# 実機での確認と計測（v1.0.0）

`v1.0.0-rc.N` の Release に添付したインストーラーと zip で、Windows の実機で確かめる。結果は下の表の「結果」の欄に書く（OK／NG と、気づいたこと）。
NG があれば直して `rc` を出し直し、すべて OK になったら v1.0.0 を出す（#14）。

用意するもの：Windows 10／11（x64、AVX2 のある CPU。2013 年ごろ以降の Intel／AMD）、Release の
`aiokey-v1.0.0-rc.N-windows.zip` と `aiokey-setup-v1.0.0-rc.N.exe`。

## 1. 登録スクリプト（zip）

1. zip を展開する（例：`C:\aiokey-rc`）。
2. PowerShell で `powershell -ExecutionPolicy Bypass -File C:\aiokey-rc\register.ps1 -Dir C:\aiokey-rc` を実行する（管理者の確認が出る）。
3. 「設定 → 時刻と言語 → 言語と地域 → 日本語 → 言語のオプション → キーボードの追加」に「AI-O Key」が出ることを確かめる。
4. `-Unregister` を付けてもう一度実行し、キーボードの一覧から消えることを確かめる。

| 項目 | 期待すること | 結果 |
|---|---|---|
| 登録 | x64 と x86 の両方で「登録しました」と出て、キーボードの追加に AI-O Key が出る | |
| 解除 | 「解除しました」と出て、一覧から消える | |

## 2. インストーラー

1. `aiokey-setup-v1.0.0-rc.N.exe` を実行する（入れる先は `%APPDATA%\aiokey`）。
2. サインアウトしてサインインし直す（ログオン時の自動起動を確かめるため）。
3. 「キーボードの追加」で AI-O Key を足し、Windows キー＋スペースで AI-O Key に切り替える。

| 項目 | 期待すること | 結果 |
|---|---|---|
| インストール | エラーなく終わる。`%APPDATA%\aiokey` に aiokey-server.exe、aiokey-ui.exe、aiokey-launcher.exe、aiokey-cli.exe、aiokey.dic、aiokey-stage2-q8_0.gguf、aiokey-stage2.json がある | |
| 自動起動 | インストールの直後と、サインインし直したあとの両方で、タスク マネージャーの「詳細」に aiokey-server と aiokey-ui がある（ないときは末尾の「動かないとき」） | |

## 3. 打って確かめる（メモ帳）

かなモード（モード表示が「あ」）で、メモ帳に打つ。

| 項目 | 操作 | 期待すること | 結果 |
|---|---|---|---|
| 子音だけの入力 | `htsthqtun` と打つ | 1 文字ごとに候補が出て、最後に「果たして本当に」が出る（Space で変換したときに候補にある） | |
| 短い入力 | `kik` と打って Space | 「機器」「危機」「開花」などが出る | |
| 英単語 | `keyboard` と打つ | 候補に「keyboard」がある | |
| 英語指定 | `/hqt` と打つ | 「hqt」がそのまま候補になる | |
| 区切り | `htst`、Shift+Space、`hqtun` と打つ | 区切りの空白が入り、「果たして」と「本当に」に分けて変換される | |
| 変換キー（段2） | 打ってすぐ Space | 段2で並べ替えた 1 位が出る | |
| 打つのを止めたとき（段2） | 打ってから手を止める | 表示が段2の 1 位に変わることがある。確信が低いときは候補ウィンドウが開く | |
| 候補ウィンドウ | Space を 2 回押す | 1 回目は開かず、2 回目で開く。番号・候補・注釈の列、1 ページ 9 件、下に n/m | |
| 選択とページ送り | ↓ で 10 件目以降へ | 選んでいる行が強調され、ページが切り替わる | |
| 数字キー | ウィンドウを開いた状態で 3 | 表示中のページの 3 番目で確定し、ウィンドウが閉じる | |
| 確定・取り消し | Enter、Esc | 確定で文字が入り、Esc で取り消す。どちらもウィンドウが閉じる | |
| モード表示 | 英数キー、かなキー、半角／全角キー | 「A」／「あ」が 1 秒ほど出て消える。英数で直接入力、かなで AI-O Key の入力に切り替わる | |
| 文脈 | 「今日は」と確定してから続けて打つ | 確定した文が文脈として効く（明らかにおかしくならなければよい） | |

## 4. 計測（基準 4、5）

PowerShell で測る。計測した値は下の表に書く。

**常駐メモリ（打鍵中）**：メモ帳で 1 分ほど打ちながら、別の PowerShell で：

```powershell
Get-Process aiokey-server, aiokey-ui | Select-Object Name, @{n='WorkingSetMB'; e={[math]::Round($_.WorkingSet64 / 1MB, 1)}}
```

**CPU 使用率（待機中）**：何も打たずに 1 分ほど置いてから、5 秒おきに 6 回測る（Windows の表示言語に左右されない測り方。
`PercentProcessorTime` は論理プロセッサ 1 つを 100 とした値なので、全体に対する割合はプロセッサの数で割る）：

```powershell
1..6 | ForEach-Object {
  Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "Name LIKE 'aiokey%'" |
    Select-Object Name, PercentProcessorTime
  Start-Sleep 5
}
```

**段2の時間（AVX2）**：温度の 3.44 は、学習済みモデルの校正値（`aiokey-stage2.json` の `temperature`）。

```powershell
$d = "$env:APPDATA\aiokey"
& "$d\aiokey-cli.exe" bench --dict "$d\aiokey.dic" --stage2 "$d\aiokey-stage2-q8_0.gguf" --temperature 3.44 --iterations 20
```

出力の `stage2 p50`、`stage2 p95`、`total p50`、`total p95` と、llama.cpp の機能の行（`AVX2 = 1` が出ていること）を書き写す。

| 項目 | 暫定目標 | 計測した値 |
|---|---|---|
| aiokey-server の WorkingSet（打鍵中） | （合計で 150 MB 以下） | |
| aiokey-ui の WorkingSet（打鍵中） | 20 MB 以下 | |
| 合計 | 150 MB 以下 | |
| CPU 使用率（待機中、2 つの合計） | ほぼ 0% | |
| 段2の p50 ／ p95（2 スレッド、上位 8 件） | 50 ms 以内（暫定。docs/spec.md の性能目標） | |
| 段1＋段2の p50 ／ p95 | 50 ms 以内（暫定。同上） | |
| llama.cpp の AVX2 | AVX2 = 1 | |
| PC の CPU とメモリ | — | |

## 動かないとき

サーバーと候補ウィンドウはランチャーが起動し、その出力と終わり方を `%LOCALAPPDATA%\aiokey\logs\launcher.log` に残す。
タスク マネージャーの「詳細」に aiokey-server と aiokey-ui がないときは、次の 2 つの結果を Issue に貼る。

```powershell
Get-Content "$env:LOCALAPPDATA\aiokey\logs\launcher.log"
schtasks /Query /TN "AI-O Key Startup" /V /FO LIST | Select-String "前回の結果|Last Result|実行するタスク|Task To Run|実行ユーザー|Run As User"
```

launcher.log がないときは、ランチャーが起動していない（タスクの登録か実行に失敗している）。
タスクを使わずに確かめるときは、`%APPDATA%\aiokey\aiokey-launcher.exe` を右クリックして「管理者として実行」する。

サーバーが後から起動しても、AI-O Key に切り替えたままキーを打てばつながる（つながるまでは英字がそのまま入る）。
