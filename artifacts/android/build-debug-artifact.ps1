[CmdletBinding()]
param(
    [string]$Version = "0.1.0-alpha",
    [string]$AndroidDir = "clients/android",
    [string]$OutDir = "artifacts/android/$Version"
)

$ErrorActionPreference = "Stop"

$apk = Join-Path $AndroidDir "app/build/outputs/apk/debug/app-debug.apk"
if (-not (Test-Path $apk)) {
    Write-Error "APK not found at $apk - run '.\gradlew assembleDebug' first."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$dest = Join-Path $OutDir "app-debug.apk"
Copy-Item $apk $dest -Force

$hash = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLower()
Set-Content -Path (Join-Path $OutDir "sha256.txt") -Value "$hash  app-debug.apk"

$gitSha = (& git rev-parse --short HEAD 2>$null)
if ($LASTEXITCODE -ne 0) { $gitSha = "unknown" }
$gitDirty = (& git status --porcelain 2>$null)
$dirty = if ($gitDirty) { $true } else { $false }
$ts = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

$buildInfo = [ordered]@{
    artifact      = "friday-console-android"
    version       = $Version
    buildType     = "debug"
    gitSha        = $gitSha
    gitDirty      = $dirty
    builtAtUtc    = $ts
    jdk           = "Adoptium JDK 17 ($env:JAVA_HOME)"
    gradle        = "8.2"
    minSdk        = 24
    targetSdk     = 34
    compileSdk    = 34
    sha256        = $hash
} | ConvertTo-Json -Compress

Set-Content -Path (Join-Path $OutDir "build-info.json") -Value $buildInfo

Write-Host "Artifact stamped: $dest"
Write-Host "sha256: $hash"
