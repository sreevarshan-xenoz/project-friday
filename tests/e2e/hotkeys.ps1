# The Windows hotkeys end to end, on the installed FRIDAY: the tray app
# running as it does after login, the keys pressed for real (SendInput, as a
# keyboard does), the question typed into the ask box, and fake_model.py as
# the only AI. Requires, per key:
#   Win+Y        the ask box opens; the answer window opens with the model's
#                answer to the typed question
#   Win+Alt+Y    the same, about text selected in Notepad (which reaches the model)
#   Win+Ctrl+Y   the same, about the screen (the model gets the screenshot)
# If another program owns a Win key, the tray falls back to Ctrl+Alt (+Shift);
# that is reported, and the fallback is pressed instead.
#
# Usage (Windows, needs an interactive desktop, as CI runners have):
#   pwsh tests/e2e/hotkeys.ps1 [-InstallDir <dir>]   (default: the MSI's)
param([string]$InstallDir = "$env:LOCALAPPDATA\Programs\FRIDAY")
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
Add-Type -AssemblyName System.Windows.Forms

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Keys {
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit, Size = 40)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT ki; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    static INPUT K(ushort vk, bool up) { var i = new INPUT { type = 1 }; i.ki.wVk = vk; i.ki.dwFlags = up ? 2u : 0u; return i; }
    // Press modifiers, tap the key, release modifiers in reverse.
    public static void Chord(ushort[] mods, ushort key) {
        var list = new System.Collections.Generic.List<INPUT>();
        foreach (var m in mods) list.Add(K(m, false));
        list.Add(K(key, false)); list.Add(K(key, true));
        for (int i = mods.Length - 1; i >= 0; i--) list.Add(K(mods[i], true));
        var a = list.ToArray();
        if (SendInput((uint)a.Length, a, Marshal.SizeOf(typeof(INPUT))) != a.Length)
            throw new Exception("SendInput failed: " + Marshal.GetLastWin32Error());
    }
    public static string ForegroundTitle() { var s = new StringBuilder(256); GetWindowText(GetForegroundWindow(), s, 256); return s.ToString(); }
    // Visible top-level windows with exactly this title.
    public static IntPtr[] Find(string title) {
        var found = new System.Collections.Generic.List<IntPtr>();
        EnumWindows((h, l) => { var s = new StringBuilder(256); GetWindowText(h, s, 256);
            if (IsWindowVisible(h) && s.ToString() == title) found.Add(h); return true; }, IntPtr.Zero);
        return found.ToArray();
    }
    public static void Close(IntPtr h) { PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); }
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    // The text of a window's first edit control (WM_GETTEXT works across
    // processes; GetWindowText doesn't for another process's controls).
    public static string EditText(IntPtr top) {
        string found = null;
        EnumChildWindows(top, (h, l) => {
            var c = new StringBuilder(256); GetClassName(h, c, 256);
            if (!c.ToString().ToUpperInvariant().Contains("EDIT")) return true;
            int len = (int)SendMessage(h, 0x000E, IntPtr.Zero, IntPtr.Zero);
            var sb = new StringBuilder(len + 1);
            SendMessage(h, 0x000D, (IntPtr)(len + 1), sb);
            found = sb.ToString();
            return false;
        }, IntPtr.Zero);
        return found ?? "";
    }
}
'@

$VK = @{ WIN = 0x5B; CTRL = 0x11; ALT = 0x12; SHIFT = 0x10; Y = 0x59; S = 0x53; A = 0x41 }
$script:fails = 0
function Pass($m) { Write-Host "  ok   $m" }
function Fail($m) { Write-Host "  FAIL $m"; $script:fails++ }
function WaitFor([scriptblock]$cond, [int]$seconds) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $end) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 200 }
    return [bool](& $cond)
}

$state = "$env:LOCALAPPDATA\friday"
$config = "$env:APPDATA\friday"
$work = Join-Path ([IO.Path]::GetTempPath()) "friday-hotkeys-$PID"
New-Item -ItemType Directory -Force $work, $config | Out-Null
# `python` first: on Windows `python3` can be the Microsoft Store stub.
$py = Get-Command python, python3 -CommandType Application -ErrorAction SilentlyContinue |
    Where-Object { $_.Source -notmatch 'WindowsApps' } | Select-Object -First 1
if (-not $py) { throw "needs Python (for the fake model)" }
# Slower streaming: the window is sampled from outside, and a slow runner
# missed the answer growing when the steps came every 0.25 s.
$env:FAKE_MODEL_PAUSE = "0.7"
$fake = Start-Process $py.Source -ArgumentList "`"$repo\tests\e2e\fake_model.py`"", "`"$work\port`"", "`"$work\model.log`"" -PassThru -WindowStyle Hidden
$tray = $null
$notepad = $null
try {
    if (-not (WaitFor { (Test-Path "$work\port") -and (Get-Content "$work\port" -Raw) } 10)) { throw "the fake model did not start" }
    $port = (Get-Content "$work\port" -Raw).Trim()
    # The tray hands friday.env to the daemon and friday-ctl: the fake model is the AI.
    # LLAMACPP_VISION: the screen ask must stay on this local model too.
    Set-Content "$config\friday.env" "LLAMACPP_ENABLED=1`nLLAMACPP_BASE_URL=http://127.0.0.1:$port/v1`nLLAMACPP_VISION=1`n"
    Remove-Item "$state\answers.md", "$state\state.json" -ErrorAction SilentlyContinue

    $tray = Start-Process "$InstallDir\friday-tray.exe" -PassThru
    if (WaitFor { (Test-Path "$state\state.json") -and ((Get-Content "$state\state.json" -Raw) -match '"status":\s*"running"') } 90) {
        Pass "tray started the daemon"
    } else { throw "the daemon never reported running (logs: $state\logs)" }
    Start-Sleep 2

    # ask <label> <keys: list of (mods, key, name)> <question> <answer pattern> [-Streams]
    # -Streams: the answer window must fill in as the answer arrives.
    function Ask($label, $chords, $question, $pattern, [switch]$Streams) {
        $used = $null
        foreach ($c in $chords) {
            [Keys]::Chord([uint16[]]$c[0], [uint16]$c[1])
            if (WaitFor { [Keys]::Find("Ask FRIDAY").Count -gt 0 } 8) { $used = $c[2]; break }
            Write-Host "       ($($c[2]) opened nothing; foreground is '$([Keys]::ForegroundTitle())')"
        }
        if (-not $used) { Fail "${label}: no key opened the ask box"; return }
        if ($used -ne $chords[0][2]) { Write-Host "       note: $($chords[0][2]) is taken on this machine; the tray fell back to $used" }
        Pass "${label}: $used opened the ask box"
        $box = [Keys]::Find("Ask FRIDAY")[0]
        [Keys]::SetForegroundWindow($box) | Out-Null
        [System.Windows.Forms.SendKeys]::SendWait($question + "{ENTER}")
        # Watch the answer window's text while waiting for the saved answer.
        $seen = [System.Collections.Generic.List[string]]::new()
        $answered = $false
        $end = (Get-Date).AddSeconds(150)
        while ((Get-Date) -lt $end) {
            foreach ($h in [Keys]::Find("FRIDAY")) {
                $t = [Keys]::EditText($h).Trim()
                if ($t -and ($seen.Count -eq 0 -or $seen[$seen.Count - 1] -ne $t)) { $seen.Add($t) }
            }
            if ((Test-Path "$state\answers.md") -and ((Get-Content "$state\answers.md" -Raw) -match [regex]::Escape("# $question"))) {
                $answered = $true; break
            }
            Start-Sleep -Milliseconds 100
        }
        $window = WaitFor { [Keys]::Find("FRIDAY").Count -gt 0 } 10
        $saved = if (Test-Path "$state\last-answer.md") { Get-Content "$state\last-answer.md" -Raw } else { "" }
        if ($answered -and $window -and $saved -match $pattern) {
            Pass "${label}: answer window shows `"$(($saved -split "`n" | Where-Object { $_ -match 'FAKE-MODEL' } | Select-Object -First 1))`""
        } else {
            Fail "${label}: expected an answer window with /$pattern/ (window: $window, saved answer below)"
            $saved -split "`n" | ForEach-Object { Write-Host "       $_" }
        }
        if ($Streams) {
            # The window showed the answer growing: at least two different
            # partial texts, each the start of the final answer.
            Start-Sleep -Milliseconds 500
            $final = ""
            foreach ($h in [Keys]::Find("FRIDAY")) { $final = [Keys]::EditText($h).Trim() }
            $partials = @($seen | Where-Object { $_ -like 'FAKE-MODEL-ANSWER*' -and $_ -ne $final })
            $growing = $partials.Count -ge 2 -and -not ($partials | Where-Object { -not $final.StartsWith($_) })
            if ($growing) { Pass "${label}: the answer window filled in ($($partials.Count) steps) before the end" }
            else {
                Fail "${label}: the answer window did not fill in as the answer streamed (seen below)"
                $seen | ForEach-Object { Write-Host "       | $_" }
            }
        }
        foreach ($h in [Keys]::Find("FRIDAY")) { [Keys]::Close($h) }
        foreach ($h in [Keys]::Find("Ask FRIDAY")) { [Keys]::Close($h) }
        Start-Sleep 1
    }

    Write-Host "presses"
    $q = "What does the Rust error E0308 mean"
    Ask "Win+Y" @((@($VK.WIN), $VK.Y, "Win+Y"), (@($VK.CTRL, $VK.ALT), $VK.Y, "Ctrl+Alt+Y")) $q `
        ("FAKE-MODEL-ANSWER: " + [regex]::Escape($q)) -Streams

    # Selected text in Notepad: the tray copies the selection itself.
    $selected = "error E0308 mismatched types expected u32 found String"
    Set-Content "$work\selection.txt" $selected
    $notepad = Start-Process notepad.exe "`"$work\selection.txt`"" -PassThru
    if (WaitFor { [Keys]::ForegroundTitle() -match 'selection' } 15) {
        [Keys]::Chord([uint16[]]@($VK.CTRL), [uint16]$VK.A)
        Start-Sleep -Milliseconds 300
        Ask "Win+Alt+Y" @((@($VK.WIN, $VK.ALT), $VK.Y, "Win+Alt+Y"), (@($VK.CTRL, $VK.ALT, $VK.SHIFT), $VK.Y, "Ctrl+Alt+Shift+Y")) `
            "explain this" "FAKE-MODEL-ANSWER: explain this" -Streams
        if ((Get-Content "$work\model.log" -Raw) -match "mismatched types expected u32") { Pass "Win+Alt+Y: the selected text reached the model" }
        else { Fail "Win+Alt+Y: the selected text never reached the model" }
    } else { Fail "Win+Alt+Y: Notepad did not come to the front (foreground: '$([Keys]::ForegroundTitle())')" }
    if ($notepad) { Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue; $notepad = $null }

    # The screen ask captures just the focused window: Notepad, not the desktop.
    Set-Content "$work\window.txt" "FRIDAY should see only this window."
    $notepad = Start-Process notepad.exe "`"$work\window.txt`"" -PassThru -WindowStyle Normal
    if (-not (WaitFor { [Keys]::ForegroundTitle() -match 'window' } 15)) {
        Fail "Win+Ctrl+Y: Notepad did not come to the front (foreground: '$([Keys]::ForegroundTitle())')"
    }
    Ask "Win+Ctrl+Y" @((@($VK.WIN, $VK.CTRL), $VK.Y, "Win+Ctrl+Y"), (@($VK.CTRL, $VK.ALT, $VK.SHIFT), $VK.S, "Ctrl+Alt+Shift+S")) `
        "what is on my screen" "FAKE-MODEL-ANSWER: what is on my screen \[saw a [1-9][0-9]*-byte [0-9]+x[0-9]+ image/png\]" -Streams
    $saved = if (Test-Path "$state\last-answer.md") { Get-Content "$state\last-answer.md" -Raw } else { "" }
    $desktop = [System.Windows.Forms.SystemInformation]::VirtualScreen
    if ($saved -match '-byte (\d+)x(\d+) image/png') {
        $w, $h = [int]$Matches[1], [int]$Matches[2]
        if ($w -gt 0 -and $h -gt 0 -and $w -lt $desktop.Width -and $h -lt $desktop.Height) {
            Pass "Win+Ctrl+Y: captured just the focused window (${w}x${h} of a $($desktop.Width)x$($desktop.Height) desktop)"
        } else { Fail "Win+Ctrl+Y: the image (${w}x${h}) is not the focused window inside the $($desktop.Width)x$($desktop.Height) desktop" }
    } else { Fail "Win+Ctrl+Y: no image size in the answer" }
    if ($notepad) { Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue; $notepad = $null }
}
catch { Fail $_ }
finally {
    # On a failure, what the desktop looked like (CI keeps it as an artifact).
    if ($script:fails -gt 0 -and $env:RUNNER_TEMP -and (Test-Path "$InstallDir\friday-windows-host.exe")) {
        cmd /c "`"$InstallDir\friday-windows-host.exe`" --screenshot > `"$env:RUNNER_TEMP\hotkeys-failure.png`""
    }
    if ($notepad) { Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue }
    # The tray's children (daemon, host) go with it; stop them too in case.
    if ($tray) { Stop-Process -Id $tray.Id -Force -ErrorAction SilentlyContinue }
    Get-Process friday-daemon, friday-windows-host -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue
    Remove-Item "$config\friday.env" -ErrorAction SilentlyContinue
}
if ($script:fails -gt 0) { Write-Host "e2e hotkeys: $($script:fails) failure(s) (work: $work, logs: $state\logs)"; exit 1 }
Write-Host "e2e hotkeys: ok"
