[CmdletBinding()]
param(
    [switch]$StopRunning,
    [switch]$Launch
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$projectDir = Join-Path $repoRoot "src_slint"
$exePath = Join-Path $projectDir "target\release\sleep_tracker.exe"

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
    Write-Host "アプリを起動します"
    Start-Process -FilePath $exePath -WorkingDirectory (Split-Path -Parent $exePath)
}
