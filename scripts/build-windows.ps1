[CmdletBinding()]
param(
    [switch]$StopRunning,
    [switch]$Launch
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$projectDir = Join-Path $repoRoot "src_slint"
$exePath = Join-Path $projectDir "target\release\sleep_tracker.exe"
$taskbarShortcut = Join-Path $env:APPDATA "Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\睡眠トラッカー.lnk"

if (-not (Test-Path -LiteralPath (Join-Path $projectDir "Cargo.toml"))) {
    throw "src_slint\Cargo.toml が見つかりません: $projectDir"
}

if ($StopRunning -or $Launch) {
    $running = @(Get-Process -Name "sleep_tracker" -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $exePath })
    foreach ($process in $running) {
        Write-Host "既存の Sleep Tracker を終了します (PID=$($process.Id))"
        Stop-Process -Id $process.Id -Force
    }

    # 名前付きMutexが解放される前に新しいプロセスを起動すると、
    # 新プロセスが二重起動側として終了し、画面なしのプロセスだけが残ることがある。
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        $remaining = @(Get-Process -Name "sleep_tracker" -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -eq $exePath })
        if ($remaining.Count -eq 0) { break }
        Start-Sleep -Milliseconds 100
    }
    if (@(Get-Process -Name "sleep_tracker" -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -eq $exePath }).Count -gt 0) {
        throw "既存の Sleep Tracker プロセスを終了できませんでした"
    }
}

Write-Host "release ビルドを実行します: $projectDir"
Push-Location $projectDir
try {
    cargo build --locked --release
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build に失敗しました (exit code=$LASTEXITCODE)"
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $exePath)) {
    throw "ビルド成果物が見つかりません: $exePath"
}

Write-Host "ビルド完了: $exePath"

if ($Launch) {
    if (-not (Test-Path -LiteralPath $taskbarShortcut)) {
        throw "タスクバーの正規ショートカットが見つかりません: $taskbarShortcut"
    }

    # exeを直接起動せず、ユーザーがダブルクリックするタスクバーの
    # .lnkをShell経由で起動する。単一インスタンス通知やタスクバー連携を
    # 正規の起動経路に揃えるため、リンク先もビルド成果物と照合する。
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($taskbarShortcut)
    $shortcutTarget = [System.IO.Path]::GetFullPath($shortcut.TargetPath)
    $expectedTarget = [System.IO.Path]::GetFullPath($exePath)
    if ($shortcutTarget -ne $expectedTarget) {
        throw "タスクバーショートカットのリンク先がrelease exeと一致しません: $shortcutTarget"
    }

    Write-Host "タスクバーの正規ショートカットを起動します: $taskbarShortcut"
    Start-Process -FilePath $taskbarShortcut
}
