# AI-O Key の TSF（aiokey.dll）を Windows に登録する・登録を解除する（開発用。ふつうはインストーラーが登録する）。
#
#   powershell -ExecutionPolicy Bypass -File scripts\register.ps1 [-Dir <フォルダ>]               登録
#   powershell -ExecutionPolicy Bypass -File scripts\register.ps1 [-Dir <フォルダ>] -Unregister   登録の解除
#
# <フォルダ> には aiokey.dll（x64）と x86\aiokey.dll（x86）を置く（Release の zip を展開したフォルダ、既定はこのスクリプトの 1 つ上の build）。
# 64 ビットのアプリは x64 の DLL を、32 ビットのアプリは x86 の DLL を読むので、両方を登録する。管理者の権限が要る。
param(
    [string]$Dir = (Join-Path $PSScriptRoot "..\build"),
    [switch]$Unregister
)
$ErrorActionPreference = "Stop"

$targets = @(
    @{ Dll = (Join-Path $Dir "aiokey.dll"); Regsvr = "$env:SystemRoot\System32\regsvr32.exe" },
    @{ Dll = (Join-Path $Dir "x86\aiokey.dll"); Regsvr = "$env:SystemRoot\SysWOW64\regsvr32.exe" }
)
foreach ($t in $targets) {
    if (-not (Test-Path $t.Dll)) { throw "$($t.Dll) が見つかりません" }
    $arguments = @("/s")
    if ($Unregister) { $arguments += "/u" }
    $arguments += "`"$($t.Dll)`""
    $p = Start-Process -FilePath $t.Regsvr -ArgumentList $arguments -Wait -PassThru -Verb RunAs
    if ($p.ExitCode -ne 0) { throw "regsvr32 $($arguments -join ' ') が失敗しました（終了コード $($p.ExitCode)）" }
    $action = if ($Unregister) { "解除" } else { "登録" }
    Write-Host "$action しました: $($t.Dll)"
}
