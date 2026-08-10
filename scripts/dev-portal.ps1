<#
.SYNOPSIS
    Boot the FRIDAY portal in one command: launch (or reuse) the Mobile Protocol
    daemon with auth off, the ReadModelBridge for the Engineering Console, then
    start the Vite dev server.

.DESCRIPTION
    The portal (frontend/) connects to the Mobile Protocol daemon at
    ws://localhost:18432. The Engineering Console (frontend/src/console) connects
    to the ReadModelBridge at ws://127.0.0.1:18433. This script makes both "just
    work":

      1. If nothing is listening on 18432, builds (if needed) and launches
         `friday-mobile-daemon` with FRIDAY_MOBILE_AUTH=off so the portal can
         connect without QR pairing.
      2. If nothing is listening on 18433 (and -SkipBridge is not given),
         builds (if needed) and launches `friday-bridge` so the Engineering
         Console is live instead of the mock.
      3. Waits for both ports to accept connections.
      4. Runs `npm run dev` in frontend/ (foreground) with VITE_CONSOLE_BRIDGE=1
         when the bridge is up (the console switches mock -> live bridge).
      5. After Vite exits, stops the services *only if this script started
         them* - an already-running service is left alone.

.PARAMETER Port
    Vite dev-server port (default 5173).

.PARAMETER Build
    Force `cargo build` of the daemon and bridge binaries even if they exist.

.PARAMETER KeepDaemon
    Leave the mobile daemon running after Vite exits.

.PARAMETER KeepBridge
    Leave the ReadModelBridge running after Vite exits.

.PARAMETER SkipBridge
    Do not boot the ReadModelBridge (daemon + Vite only; console uses the mock).

.EXAMPLE
    .\scripts\dev-portal.ps1                          # portal on :5173 + bridge
    .\scripts\dev-portal.ps1 -Port 5175               # portal on :5175 + bridge
    .\scripts\dev-portal.ps1 -Build                   # force rebuild
    .\scripts\dev-portal.ps1 -KeepDaemon -KeepBridge  # keep both services up
    .\scripts\dev-portal.ps1 -SkipBridge              # console uses the mock
#>
param(
    [int]$Port = 5173,
    [switch]$Build,
    [switch]$KeepDaemon,
    [switch]$KeepBridge,
    [switch]$SkipBridge
)

$ErrorActionPreference = "Stop"

# Repo root = the script's parent's parent.
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DaemonBin = Join-Path $Root "target\debug\friday-mobile-daemon.exe"
$BridgeBin = Join-Path $Root "target\debug\friday-bridge.exe"
$DaemonLogOut = Join-Path $Root "target\mobile-daemon.out.log"
$DaemonLogErr = Join-Path $Root "target\mobile-daemon.err.log"
$BridgeLogOut = Join-Path $Root "target\bridge.out.log"
$BridgeLogErr = Join-Path $Root "target\bridge.err.log"
$MobilePort = 18432
$BridgePort = 18433

function Test-Port($port) {
    return [bool](Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue)
}

function Ensure-Binary($path, $crate, $bin, $label) {
    if (-not (Test-Path $path) -or $Build) {
        Write-Host "[portal] Building $label ($crate::$bin) ..."
        Push-Location $Root
        try {
            & cargo build -p $crate --bin $bin
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $label (exit $LASTEXITCODE)" }
        } finally {
            Pop-Location
        }
    }
}

# ---- Mobile daemon (:18432) -------------------------------------------------
$startedByUs = $false
if (Test-Port $MobilePort) {
    Write-Host "[portal] Mobile daemon already running on :$MobilePort - reusing it." -ForegroundColor Green
} else {
    Ensure-Binary $DaemonBin "friday-mobile" "friday-mobile-daemon" "friday-mobile-daemon"
    Write-Host "[portal] Launching daemon with FRIDAY_MOBILE_AUTH=off ..."
    # Child inherits the env at spawn time; restore immediately after.
    $prevAuth = $env:FRIDAY_MOBILE_AUTH
    $env:FRIDAY_MOBILE_AUTH = "off"
    $proc = Start-Process -FilePath $DaemonBin -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $DaemonLogOut -RedirectStandardError $DaemonLogErr
    $env:FRIDAY_MOBILE_AUTH = $prevAuth
    $startedByUs = $true

    # Wait up to 30 s for the port to accept connections.
    $waited = 0
    while (-not (Test-Port $MobilePort) -and $waited -lt 30) {
        Start-Sleep -Seconds 1
        $waited++
        if ($proc.HasExited) {
            Write-Host "[portal] Daemon exited early - check $DaemonLogErr" -ForegroundColor Red
            throw "friday-mobile-daemon exited (code $($proc.ExitCode))"
        }
    }
    if (-not (Test-Port $MobilePort)) {
        # The process may still be running even though it never opened the
        # port - don't leave it orphaned.
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        throw "daemon did not open :$MobilePort within 30 s - check $DaemonLogErr"
    }
    Write-Host "[portal] Mobile daemon listening on :$MobilePort (log: target/mobile-daemon.out.log)" -ForegroundColor Green
}

# ---- ReadModelBridge (:18433) -----------------------------------------------
$bridgeStartedByUs = $false
$bridgeUp = Test-Port $BridgePort
if ($SkipBridge) {
    Write-Host "[portal] Skipping ReadModelBridge (-SkipBridge) - console will use the mock."
} elseif ($bridgeUp) {
    Write-Host "[portal] ReadModelBridge already running on :$BridgePort - reusing it." -ForegroundColor Green
} else {
    # If any later boot step throws, stop what THIS script started so a failed
    # boot never leaves an orphaned daemon/bridge behind.
    try {
        Ensure-Binary $BridgeBin "friday-bridge" "friday-bridge" "friday-bridge"
        Write-Host "[portal] Launching ReadModelBridge ..."
        $bridgeProc = Start-Process -FilePath $BridgeBin -PassThru -WindowStyle Hidden `
            -RedirectStandardOutput $BridgeLogOut -RedirectStandardError $BridgeLogErr
        $bridgeStartedByUs = $true

        # Wait up to 30 s for the port to accept connections.
        $waited = 0
        while (-not (Test-Port $BridgePort) -and $waited -lt 30) {
            Start-Sleep -Seconds 1
            $waited++
            if ($bridgeProc.HasExited) {
                Write-Host "[portal] ReadModelBridge exited early - check $BridgeLogErr" -ForegroundColor Red
                throw "friday-bridge exited (code $($bridgeProc.ExitCode))"
            }
        }
        if (-not (Test-Port $BridgePort)) {
            throw "friday-bridge did not open :$BridgePort within 30 s - check $BridgeLogErr"
        }
        Write-Host "[portal] ReadModelBridge listening on :$BridgePort (log: target/bridge.out.log)" -ForegroundColor Green
    } catch {
        if ($startedByUs) {
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        }
        if ($bridgeStartedByUs) {
            Stop-Process -Id $bridgeProc.Id -Force -ErrorAction SilentlyContinue
        }
        throw
    }
}

Write-Host "[portal] Starting Vite dev server on :$Port ..." -ForegroundColor Cyan
Push-Location (Join-Path $Root "frontend")
$prevConsoleBridge = $env:VITE_CONSOLE_BRIDGE
try {
    # VITE_CONSOLE_BRIDGE=1 switches the Engineering Console from the mock to
    # the live ReadModelBridge (only when the bridge is up; restore the user's
    # own value afterwards).
    if (-not $SkipBridge -and (Test-Port $BridgePort)) {
        $env:VITE_CONSOLE_BRIDGE = "1"
    }
    # Use npm.cmd explicitly: the npm.ps1 shim shipped with Node.js mangles
    # arguments in PowerShell 5.1 ("Unknown command: pm"). --strictPort makes
    # a busy port fail loudly instead of silently moving the portal.
    & npm.cmd run dev -- --port $Port --strictPort
} finally {
    if ($null -eq $prevConsoleBridge) {
        Remove-Item Env:VITE_CONSOLE_BRIDGE -ErrorAction SilentlyContinue
    } else {
        $env:VITE_CONSOLE_BRIDGE = $prevConsoleBridge
    }
    Pop-Location
}

# Cleanup: stop only the services this script started (unless the matching
# -Keep* switch is given); an already-running service is never touched.
if ($startedByUs -and -not $KeepDaemon) {
    Write-Host "[portal] Vite exited - stopping the daemon this script started (use -KeepDaemon to leave it up)."
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
} elseif ($startedByUs -and $KeepDaemon) {
    Write-Host "[portal] Leaving the daemon running on :$MobilePort."
}
if ($bridgeStartedByUs -and -not $KeepBridge) {
    Write-Host "[portal] Vite exited - stopping the ReadModelBridge this script started (use -KeepBridge to leave it up)."
    Stop-Process -Id $bridgeProc.Id -Force -ErrorAction SilentlyContinue
} elseif ($bridgeStartedByUs -and $KeepBridge) {
    Write-Host "[portal] Leaving the ReadModelBridge running on :$BridgePort."
}

# Propagate Vite's exit code (cleanup has already run, so this is safe on Ctrl-C too).
exit $LASTEXITCODE
