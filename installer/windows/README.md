# FRIDAY for Windows: the installer

`FRIDAY-<version>-x64.msi` is a **per-user** install: no admin prompt, and
nothing outside the user's profile.

| What | Where |
|---|---|
| Programs | `%LOCALAPPDATA%\Programs\FRIDAY` (`friday-tray.exe`, `friday-daemon.exe`, `friday-ctl.exe`, `friday-windows-host.exe`, and their .NET runtime) |
| Starts at login (option, on) | `HKCU\...\CurrentVersion\Run\FRIDAY` (the tray's "Start at login" toggles it) |
| Start menu | `FRIDAY` |
| `friday-ctl` in terminals (option, on) | the install folder is added to the user `PATH` |
| Desktop shortcut (option, off) | `FRIDAY` on the desktop |
| Read a window's text for screen questions (option, on) | on by default in the tray too; the tray menu turns it off (`FRIDAY_SCREEN_TEXT=0` in `friday.env`) |
| State, answers, logs | `%LOCALAPPDATA%\friday` (kept on uninstall) |
| AI provider key | `%APPDATA%\friday\friday.env` (kept on uninstall) |

FRIDAY runs in the user's session, not as a Windows service. A service runs
in session 0, where it can't see the desktop: it can't read UI Automation,
capture the screen, hold a hotkey or show a window. The tray app
(`windows-tray/`) starts at login, owns the hotkeys and the ask box, and
keeps `friday-daemon` and `friday-windows-host` running. If they crash it
restarts them, and they never outlive it.

## Options on the first install

Running the MSI shows a Welcome page, a **Customize** page and a last page with
"Start FRIDAY now". The Customize page lists the options above with a switch and
a description each. A silent install shows nothing and takes the defaults (what
winget does); to choose on the command line, list the features to install:

```powershell
# everything on by default, plus the desktop shortcut
msiexec /i FRIDAY-<version>-x64.msi /qn ADDLOCAL=Main,StartAtLogin,PathEntry,DesktopShortcut
# only the program: no start at login, no PATH change
msiexec /i FRIDAY-<version>-x64.msi /qn ADDLOCAL=Main
```

Features: `Main` (required), `StartAtLogin`, `PathEntry`, `DesktopShortcut`,
`ScreenText`, `AdminAccess`. The last starts FRIDAY with administrator rights (so it can
control apps that run as administrator): a per-user install can't elevate itself, so
the tray asks Windows once when it first starts after setup and creates a sign-in
task; it can be turned on or off any time in Settings. The dialog images are drawn by `assets/make-assets.py`.

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
and check it cleaned up; then a second install with only some options
(`ADDLOCAL`) to check that an option left out is left out.

## Releases

Pushing a tag `vX.Y.Z` runs `.github/workflows/release.yml`:
- it builds and smoke-tests the Arch package and the MSI (versioned X.Y.Z);
- it attaches both to a GitHub release, with `SHA256SUMS` and a zip of the
  winget manifests already pointing at the release's MSI.

CI builds are numbered 0.1.<commits>, so release 0.2.0 or later.

## winget

winget installs from a public URL, so its manifests are generated per
release. Publish the MSI (e.g. as a GitHub release asset), then:

```powershell
pwsh installer\windows\winget\New-WingetManifest.ps1 -Msi out\FRIDAY-<version>-x64.msi -Url <public URL of that MSI>
winget validate --manifest installer\windows\winget\manifests\p\ProjectFRIDAY\FRIDAY\<version>
```

The script writes the three manifest files (version, installer, locale) for
`ProjectFRIDAY.FRIDAY`: a per-user MSI, silent-capable, with `friday-ctl` on
`PATH`. Submitting them is a pull request to `microsoft/winget-pkgs`.
