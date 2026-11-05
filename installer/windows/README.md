# FRIDAY for Windows: the installer

`FRIDAY-<version>-x64.msi` is a **per-user** install: no admin prompt, and
nothing outside the user's profile.

| What | Where |
|---|---|
| Programs | `%LOCALAPPDATA%\Programs\FRIDAY` (`friday-tray.exe`, `friday-daemon.exe`, `friday-ctl.exe`, `friday-windows-host.exe`, and their .NET runtime) |
| Starts at login | `HKCU\...\CurrentVersion\Run\FRIDAY` (the tray's "Start at login" toggles it) |
| Start menu | `FRIDAY` |
| `friday-ctl` in terminals | the install folder is added to the user `PATH` |
| State, answers, logs | `%LOCALAPPDATA%\friday` (kept on uninstall) |
| AI provider key | `%APPDATA%\friday\friday.env` (kept on uninstall) |

FRIDAY runs in the user's session, not as a Windows service. A service runs
in session 0, where it can't see the desktop: it can't read UI Automation,
capture the screen, hold a hotkey or show a window. The tray app
(`windows-tray/`) starts at login, owns the hotkeys and the ask box, and
keeps `friday-daemon` and `friday-windows-host` running. If they crash it
restarts them, and they never outlive it.

## Build

On Windows with Rust (MSVC), the .NET 8 SDK and PowerShell 7:

```powershell
pwsh installer\windows\build.ps1        # -> installer\windows\out\FRIDAY-<version>-x64.msi
```

The script:
1. builds the daemon and `friday-ctl` with cargo;
2. publishes the tray and host self-contained (the target needs no .NET);
3. runs WiX v5 over `FRIDAY.wxs`. The WiX dotnet tool is installed on first use.

## Install, check, uninstall

```powershell
msiexec /i FRIDAY-<version>-x64.msi            # interactive: starts the tray when done
msiexec /i FRIDAY-<version>-x64.msi /qn        # silent (CI, winget)
& "$env:LOCALAPPDATA\Programs\FRIDAY\friday-tray.exe" --smoke   # daemon + host up, friday-ctl reaches it
msiexec /x FRIDAY-<version>-x64.msi /qn
```

CI (`windows-installer` job) does exactly this on every push: build, silent
install, check files, Run entry, PATH and shortcut, smoke test, uninstall,
and check it cleaned up.
