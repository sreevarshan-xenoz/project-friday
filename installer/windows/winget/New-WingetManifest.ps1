<#
.SYNOPSIS
  Write the winget manifests for a FRIDAY release.

.DESCRIPTION
  winget installs from a public URL, so manifests are made per release:
  give the MSI built by build.ps1 and the URL it is published at (e.g. a
  GitHub release asset). Writes the three manifest files (version,
  installer, defaultLocale) under manifests\<id path>\<version>, ready for
  `winget validate` and a pull request to microsoft/winget-pkgs.

.EXAMPLE
  pwsh New-WingetManifest.ps1 -Msi ..\out\FRIDAY-0.1.180-x64.msi `
    -Url https://github.com/sreevarshan-xenoz/project-friday/releases/download/v0.1.180/FRIDAY-0.1.180-x64.msi
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Msi,
    [Parameter(Mandatory)] [string]$Url,
    [string]$Version,
    [string]$OutDir = "$PSScriptRoot\manifests"
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$id = 'ProjectFRIDAY.FRIDAY'
if (-not $Version) {
    if ((Split-Path $Msi -Leaf) -notmatch '^FRIDAY-([0-9.]+)-x64\.msi$') { throw "pass -Version (can't read it from $Msi)" }
    $Version = $Matches[1]
}
$sha = (Get-FileHash -Algorithm SHA256 $Msi).Hash
$dir = Join-Path $OutDir ("p\ProjectFRIDAY\FRIDAY\$Version")
New-Item -ItemType Directory -Force $dir | Out-Null
$schema = '1.6.0'

@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@ | Set-Content -Encoding utf8 "$dir\$id.yaml"

@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
Platform:
- Windows.Desktop
MinimumOSVersion: 10.0.17763.0
InstallerType: msi
Scope: user
InstallModes:
- interactive
- silent
UpgradeBehavior: install
Commands:
- friday-ctl
Installers:
- Architecture: x64
  InstallerUrl: $Url
  InstallerSha256: $sha
ManifestType: installer
ManifestVersion: $schema
"@ | Set-Content -Encoding utf8 "$dir\$id.installer.yaml"

@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: Project FRIDAY
PublisherUrl: https://github.com/sreevarshan-xenoz/project-friday
PackageName: FRIDAY
PackageUrl: https://github.com/sreevarshan-xenoz/project-friday
License: MIT OR Apache-2.0
LicenseUrl: https://github.com/sreevarshan-xenoz/project-friday/blob/dev/LICENSE-MIT
ShortDescription: Press a key, ask about what's on screen or selected, get an answer.
Description: |-
  FRIDAY is a desktop assistant. Press Win+Y to ask anything, Win+Alt+Y to ask about the
  selected text, or Win+Ctrl+Y to ask about the screen. It runs per user from the tray and
  uses a free cloud model by default, or your own AI provider key.
Tags:
- assistant
- ai
- productivity
ManifestType: defaultLocale
ManifestVersion: $schema
"@ | Set-Content -Encoding utf8 "$dir\$id.locale.en-US.yaml"

Write-Host "Wrote winget manifests for $id $Version to $dir"
Write-Host "Check with: winget validate --manifest `"$dir`""
