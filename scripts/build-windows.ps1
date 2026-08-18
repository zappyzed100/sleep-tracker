[CmdletBinding()]
param(
    [switch]$StopRunning,
    [switch]$Launch
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$projectDir = Join-Path $repoRoot "src_slint"
$exePath = Join-Path $projectDir "target\release\sleep_tracker.exe"
$startMenuDirectory = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$appDisplayName = ([char]0x7761).ToString() + [char]0x7720 + [char]0x30C8 + [char]0x30E9 + [char]0x30C3 + [char]0x30AB + [char]0x30FC
$startMenuShortcut = Join-Path $startMenuDirectory ($appDisplayName + ".lnk")

if (-not (Test-Path -LiteralPath (Join-Path $projectDir "Cargo.toml"))) {
    throw "src_slint\Cargo.toml が見つかりません: $projectDir"
}

function Register-StartMenuShortcut {
    param(
        [Parameter(Mandatory = $true)][string]$ShortcutPath,
        [Parameter(Mandatory = $true)][string]$TargetPath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $parentDirectory = Split-Path -Parent $ShortcutPath
    New-Item -ItemType Directory -Path $parentDirectory -Force | Out-Null

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($ShortcutPath)
    $shortcut.TargetPath = $TargetPath
    $shortcut.WorkingDirectory = $WorkingDirectory
    $shortcut.Description = "Sleep Tracker - sleep logging and analysis"
    $shortcut.IconLocation = [string]::Concat($TargetPath, ",0")
    $shortcut.Save()

    $savedShortcut = $shell.CreateShortcut($ShortcutPath)
    $savedTargetRaw = [string]$savedShortcut.TargetPath
    $expectedTarget = [System.IO.Path]::GetFullPath([string]$TargetPath)
    if ([string]::IsNullOrWhiteSpace($savedTargetRaw)) {
        throw "Start Menu shortcut target is empty"
    }
    $savedTarget = [System.IO.Path]::GetFullPath($savedTargetRaw)
    if ($savedTarget -ne $expectedTarget) {
        throw "Start Menu shortcut target mismatch: $savedTarget"
    }
    $savedIconLocation = [string]$savedShortcut.IconLocation
    if ($savedIconLocation -ne ([string]::Concat($expectedTarget, ",0"))) {
        throw "Start Menu shortcut icon mismatch: $savedIconLocation"
    }
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

$workingDirectory = Split-Path -Parent $exePath
Register-StartMenuShortcut `
    -ShortcutPath $startMenuShortcut `
    -TargetPath $exePath `
    -WorkingDirectory $workingDirectory
Write-Host "Registered Start Menu shortcut: $startMenuShortcut"

if ($Launch) {
    Write-Host "Launching Start Menu shortcut: $startMenuShortcut"
    # .lnkをStart-Processに渡すと、Windowsのショートカット解決を使って起動できる。
    # Shell.Application.InvokeVerb()はCOMサーバー経由の起動となり、起動後に
    # アプリのHWNDだけが破棄される環境があるため使用しない。
    Start-Process -FilePath $startMenuShortcut
    # ショートカット起動は非同期なので、初期ウィンドウ作成を待つ。
    Start-Sleep -Seconds 2
}
