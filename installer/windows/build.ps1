<#
.SYNOPSIS
  Build the FRIDAY MSI for Windows (per-user, x64).

.DESCRIPTION
  1. cargo builds friday-linux-daemon (shipped as friday-daemon.exe) and
     friday-ctl in release mode;
  2. publishes friday-tray and friday-windows-host self-contained (no .NET
     install needed on the target), into one staging folder;
  3. runs WiX v5 over installer\windows\FRIDAY.wxs.
  Needs: Rust (MSVC), the .NET 8 SDK, and the WiX v5 dotnet tool (installed
  on first run). Run from anywhere; paths are relative to the repo.

.EXAMPLE
  pwsh installer\windows\build.ps1            # -> installer\windows\out\FRIDAY-<version>-x64.msi
#>
[CmdletBinding()]
param(
    [string]$OutDir = "$PSScriptRoot\out",
    [switch]$SkipCargo
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$stage = Join-Path $OutDir 'stage'
$wixVersion = '5.0.2'

function Run([string]$exe, [string[]]$arguments) {
    Write-Host "> $exe $($arguments -join ' ')"
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "$exe failed with exit code $LASTEXITCODE" }
}

# MSI ProductVersion: major.minor.build, each build distinct so upgrades apply.
$crateVersion = (Select-String -Path "$repo\services\mobile\Cargo.toml" -Pattern '^version\s*=\s*"([0-9]+)\.([0-9]+)\.' |
    Select-Object -First 1).Matches[0].Groups
$commits = (git -C $repo rev-list --count HEAD).Trim()
$version = "$($crateVersion[1].Value).$($crateVersion[2].Value).$commits"
Write-Host "FRIDAY $version"

Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null

if (-not $SkipCargo) {
    Run cargo @('build', '--release', '--manifest-path', "$repo\Cargo.toml",
        '--package', 'friday-mobile', '--bin', 'friday-linux-daemon', '--bin', 'friday-ctl')
}
Copy-Item "$repo\target\release\friday-linux-daemon.exe" "$stage\friday-daemon.exe"
Copy-Item "$repo\target\release\friday-ctl.exe" "$stage\friday-ctl.exe"

foreach ($project in @("$repo\windows-tray\FridayTray\FridayTray.csproj", "$repo\windows-host\windows-host.csproj")) {
    Run dotnet @('publish', $project, '-c', 'Release', '-r', 'win-x64', '--self-contained', 'true',
        '-p:PublishSingleFile=false', '-o', $stage)
}
Copy-Item "$repo\LICENSE-MIT", "$repo\LICENSE-APACHE" $stage
# Debug symbols stay out of the installer.
Get-ChildItem $stage -Filter *.pdb -Recurse | Remove-Item

foreach ($exe in 'friday-tray.exe', 'friday-daemon.exe', 'friday-ctl.exe', 'friday-windows-host.exe') {
    if (-not (Test-Path "$stage\$exe")) { throw "staging is missing $exe" }
}

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    Run dotnet @('tool', 'install', '--global', 'wix', '--version', $wixVersion)
    $env:PATH += ";$env:USERPROFILE\.dotnet\tools"
}
Run wix @('extension', 'add', '--global', "WixToolset.Util.wixext/$wixVersion")

$msi = Join-Path $OutDir "FRIDAY-$version-x64.msi"
Run wix @('build', "$PSScriptRoot\FRIDAY.wxs", '-arch', 'x64', '-ext', 'WixToolset.Util.wixext',
    '-bindpath', "stage=$stage", '-d', "Version=$version", '-o', $msi)
Write-Host "MSI: $msi"
"msi=$msi" | Out-File -Append -Encoding utf8 ($env:GITHUB_OUTPUT ?? 'NUL')
