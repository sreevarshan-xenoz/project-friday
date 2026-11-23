using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Windows.Automation;
using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// Installs FRIDAY's unpacked browser extension into Chromium browsers
/// (Chrome, Edge, Brave, Vivaldi) through UI Automation.
/// </summary>
internal static class ExtensionInstaller
{
    public static async Task<string> InstallAsync(KnownBrowser b, Install install, CancellationToken ct)
    {
        if (b.Firefox)
            return BrowserLink.Steps(b, BrowserSetup.ExtensionDir(install));

        // The caller puts the folder on the clipboard first, on its own (STA)
        // thread: this runs on a background thread, where the clipboard can't.
        var folder = BrowserSetup.ExtensionDir(install);

        try
        {
            BrowserSetup.RegisterHost(b, install);
        }
        catch (Exception)
        {
            return BrowserLink.FailureMessage(ExtensionInstallStep.Window, b);
        }

        var exe = BrowserSetup.ExePath(b);
        if (exe is null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.Window, b);

        // Only the window this starts is touched: the person's open windows
        // keep their tabs, and a tab of theirs titled "Extensions" isn't taken
        // for the extensions page.
        var targetProc = Path.GetFileNameWithoutExtension(b.Exe);
        var before = BrowserWindows(targetProc);

        try
        {
            Process.Start(new ProcessStartInfo(exe, $"--no-first-run --new-window \"{b.ExtensionsPage}\"") { UseShellExecute = false });
        }
        catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or IOException)
        {
            return BrowserLink.FailureMessage(ExtensionInstallStep.Window, b);
        }

        // Wait up to 15 s for a top-level window of that browser process whose name contains "Extensions".
        AutomationElement? extWindow = null;
        var windowDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(15);
        var typed = false;
        while (DateTime.UtcNow < windowDeadline)
        {
            ct.ThrowIfCancellationRequested();
            extWindow = FindExtensionsWindow(targetProc, before);
            if (extWindow != null)
                break;
            // Browsers ignore their own pages (edge://extensions) given on the
            // command line and open a new tab: type the address there instead.
            if (!typed && OpenInAddressBar(targetProc, before, b.ExtensionsPage))
                typed = true;
            await Task.Delay(250, ct);
        }

        if (extWindow == null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.Window, b);

        // A narrow window hides Developer mode behind the page's menu button.
        try
        {
            if (extWindow.TryGetCurrentPattern(WindowPattern.Pattern, out var wp))
                ((WindowPattern)wp).SetWindowVisualState(WindowVisualState.Maximized);
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }

        // Find Developer mode toggle: ControlType.Button or CheckBox with TogglePattern.
        TogglePattern? devToggle = null;
        var devDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(10);
        while (DateTime.UtcNow < devDeadline)
        {
            ct.ThrowIfCancellationRequested();
            devToggle = FindDeveloperModeToggle(extWindow);
            if (devToggle != null)
                break;
            await Task.Delay(250, ct);
        }

        if (devToggle == null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.DeveloperMode, b);

        try
        {
            if (devToggle.Current.ToggleState != ToggleState.On)
            {
                devToggle.Toggle();
                var toggleDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(5);
                while (DateTime.UtcNow < toggleDeadline)
                {
                    ct.ThrowIfCancellationRequested();
                    if (devToggle.Current.ToggleState == ToggleState.On)
                        break;
                    await Task.Delay(250, ct);
                }

                if (devToggle.Current.ToggleState != ToggleState.On)
                    return BrowserLink.FailureMessage(ExtensionInstallStep.DeveloperMode, b);
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
            return BrowserLink.FailureMessage(ExtensionInstallStep.DeveloperMode, b);
        }

        // Find the button named "Load unpacked" (InvokePattern) and Invoke it.
        InvokePattern? loadUnpacked = null;
        var loadDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(10);
        while (DateTime.UtcNow < loadDeadline)
        {
            ct.ThrowIfCancellationRequested();
            loadUnpacked = FindLoadUnpackedButton(extWindow);
            if (loadUnpacked != null)
                break;
            await Task.Delay(250, ct);
        }

        if (loadUnpacked == null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.LoadUnpacked, b);

        try
        {
            loadUnpacked.Invoke();
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
            return BrowserLink.FailureMessage(ExtensionInstallStep.LoadUnpacked, b);
        }

        // Wait up to 10 s for the folder dialog.
        AutomationElement? folderDialog = null;
        var dialogDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(10);
        while (DateTime.UtcNow < dialogDeadline)
        {
            ct.ThrowIfCancellationRequested();
            folderDialog = FindFolderDialog(targetProc);
            if (folderDialog != null)
                break;
            await Task.Delay(250, ct);
        }

        if (folderDialog == null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.FolderDialog, b);

        // Find the edit box for the folder and set its value to the extension directory.
        ValuePattern? folderInput = null;
        var inputDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(5);
        while (DateTime.UtcNow < inputDeadline)
        {
            ct.ThrowIfCancellationRequested();
            folderInput = FindFolderInput(folderDialog);
            if (folderInput != null)
                break;
            await Task.Delay(250, ct);
        }

        if (folderInput == null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.FolderInput, b);

        try
        {
            folderInput.SetValue(folder);
            // Read back: the dialog's box can ignore a value set from outside.
            if (!string.Equals(folderInput.Current.Value?.Trim(), folder, StringComparison.OrdinalIgnoreCase))
                return BrowserLink.FailureMessage(ExtensionInstallStep.FolderInput, b);
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
            return BrowserLink.FailureMessage(ExtensionInstallStep.FolderInput, b);
        }

        // Invoke the button named "Select Folder".
        InvokePattern? selectBtn = null;
        var selectDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(5);
        while (DateTime.UtcNow < selectDeadline)
        {
            ct.ThrowIfCancellationRequested();
            selectBtn = FindSelectFolderButton(folderDialog);
            if (selectBtn != null)
                break;
            await Task.Delay(250, ct);
        }

        if (selectBtn == null)
            return BrowserLink.FailureMessage(ExtensionInstallStep.SelectFolder, b);

        try
        {
            selectBtn.Invoke();
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
            return BrowserLink.FailureMessage(ExtensionInstallStep.SelectFolder, b);
        }

        // Verify: within 10 s the extensions page shows an element whose Name contains "FRIDAY".
        var verifyDeadline = DateTime.UtcNow + TimeSpan.FromSeconds(10);
        while (DateTime.UtcNow < verifyDeadline)
        {
            ct.ThrowIfCancellationRequested();
            if (ShowsExtension(extWindow) || (FindExtensionsWindow(targetProc, before) is { } win && ShowsExtension(win)))
                return BrowserLink.SuccessMessage(b);
            await Task.Delay(250, ct);
        }

        return BrowserLink.FailureMessage(ExtensionInstallStep.Verify, b);
    }

    /// <summary>Types <paramref name="address"/> into the browser window's address bar and
    /// presses Enter. False until there is a new window with an address bar, in front.</summary>
    private static bool OpenInAddressBar(string targetProc, HashSet<int> before, string address)
    {
        try
        {
            var windows = AutomationElement.RootElement.FindAll(
                TreeScope.Children,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window));
            foreach (AutomationElement win in windows)
            {
                if (!IsMatchingProcess(win.Current.ProcessId, targetProc)
                    || before.Contains(win.Current.NativeWindowHandle))
                    continue;
                // Enter goes to whatever window is in front: only press it
                // when that is the new browser window.
                var hwnd = new IntPtr(win.Current.NativeWindowHandle);
                Native.SetForegroundWindow(hwnd);
                if (Native.GetForegroundWindow() != hwnd)
                    return false;
                var edits = win.FindAll(TreeScope.Descendants,
                    new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Edit));
                foreach (AutomationElement edit in edits)
                {
                    var name = edit.Current.Name ?? "";
                    // "Address and search bar" (Chrome, Edge, Brave), "Address field" (Vivaldi).
                    if (!name.Contains("Address", StringComparison.OrdinalIgnoreCase)
                        || !edit.TryGetCurrentPattern(ValuePattern.Pattern, out var vp))
                        continue;
                    edit.SetFocus();
                    ((ValuePattern)vp).SetValue(address);
                    PressEnter();
                    return true;
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return false;
    }

    private static void PressEnter()
    {
        static Native.INPUT Key(bool up) => new()
        {
            type = Native.INPUT_KEYBOARD,
            ki = new Native.KEYBDINPUT { wVk = 0x0D, dwFlags = up ? Native.KEYEVENTF_KEYUP : 0 },
        };
        Native.INPUT[] inputs = [Key(false), Key(true)];
        Native.SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<Native.INPUT>());
    }

    /// <summary>Handles of the browser's top-level windows open now.</summary>
    private static HashSet<int> BrowserWindows(string targetProc)
    {
        var handles = new HashSet<int>();
        try
        {
            var windows = AutomationElement.RootElement.FindAll(
                TreeScope.Children,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window));
            foreach (AutomationElement win in windows)
            {
                try
                {
                    if (IsMatchingProcess(win.Current.ProcessId, targetProc))
                        handles.Add(win.Current.NativeWindowHandle);
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return handles;
    }

    private static AutomationElement? FindExtensionsWindow(string targetProc, HashSet<int> before)
    {
        try
        {
            var windows = AutomationElement.RootElement.FindAll(
                TreeScope.Children,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window));

            foreach (AutomationElement win in windows)
            {
                try
                {
                    if (!BrowserLink.IsExtensionsWindow(win.Current.Name))
                        continue;

                    if (IsMatchingProcess(win.Current.ProcessId, targetProc)
                        && !before.Contains(win.Current.NativeWindowHandle))
                        return win;
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return null;
    }

    private static TogglePattern? FindDeveloperModeToggle(AutomationElement window)
    {
        try
        {
            var condition = new OrCondition(
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button),
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.CheckBox));

            var elements = window.FindAll(TreeScope.Descendants, condition);
            foreach (AutomationElement el in elements)
            {
                try
                {
                    var name = el.Current.Name;
                    if (!string.IsNullOrEmpty(name) && name.Contains("Developer mode", StringComparison.OrdinalIgnoreCase))
                    {
                        if (el.TryGetCurrentPattern(TogglePattern.Pattern, out var pat) && pat is TogglePattern toggle)
                            return toggle;
                    }
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }

            // Fallback for custom controls exposing TogglePattern with Developer mode in Name
            var all = window.FindAll(TreeScope.Descendants, Condition.TrueCondition);
            foreach (AutomationElement el in all)
            {
                try
                {
                    var name = el.Current.Name;
                    if (!string.IsNullOrEmpty(name) && name.Contains("Developer mode", StringComparison.OrdinalIgnoreCase))
                    {
                        if (el.TryGetCurrentPattern(TogglePattern.Pattern, out var pat) && pat is TogglePattern toggle)
                            return toggle;
                    }
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return null;
    }

    private static InvokePattern? FindLoadUnpackedButton(AutomationElement window)
    {
        try
        {
            var buttons = window.FindAll(
                TreeScope.Descendants,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button));

            foreach (AutomationElement btn in buttons)
            {
                try
                {
                    var name = btn.Current.Name;
                    if (!string.IsNullOrEmpty(name) && name.Contains("Load unpacked", StringComparison.OrdinalIgnoreCase))
                    {
                        if (btn.TryGetCurrentPattern(InvokePattern.Pattern, out var pat) && pat is InvokePattern invoke)
                            return invoke;
                    }
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }

            var all = window.FindAll(TreeScope.Descendants, Condition.TrueCondition);
            foreach (AutomationElement el in all)
            {
                try
                {
                    var name = el.Current.Name;
                    if (!string.IsNullOrEmpty(name) && name.Contains("Load unpacked", StringComparison.OrdinalIgnoreCase))
                    {
                        if (el.TryGetCurrentPattern(InvokePattern.Pattern, out var pat) && pat is InvokePattern invoke)
                            return invoke;
                    }
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return null;
    }

    private static AutomationElement? FindFolderDialog(string targetProc)
    {
        try
        {
            var windows = AutomationElement.RootElement.FindAll(
                TreeScope.Children,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window));

            foreach (AutomationElement win in windows)
            {
                try
                {
                    if (!IsMatchingProcess(win.Current.ProcessId, targetProc))
                        continue;

                    if (BrowserLink.IsFolderPickerDialog(win.Current.Name, win.Current.ClassName))
                        return win;
                    // A dialog the browser owns is listed under its window, not at the top.
                    foreach (AutomationElement child in win.FindAll(TreeScope.Children,
                        new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window)))
                    {
                        if (BrowserLink.IsFolderPickerDialog(child.Current.Name, child.Current.ClassName))
                            return child;
                    }
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return null;
    }

    private static ValuePattern? FindFolderInput(AutomationElement dialog)
    {
        try
        {
            var condition = new OrCondition(
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Edit),
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.ComboBox));

            var elements = dialog.FindAll(TreeScope.Descendants, condition);
            ValuePattern? fallback = null;

            foreach (AutomationElement el in elements)
            {
                try
                {
                    if (!el.TryGetCurrentPattern(ValuePattern.Pattern, out var pat) || pat is not ValuePattern valPattern)
                        continue;

                    var id = el.Current.AutomationId;
                    var name = el.Current.Name;

                    if (id == "1152" || (!string.IsNullOrEmpty(name) && name.Contains("Folder", StringComparison.OrdinalIgnoreCase)))
                        return valPattern;

                    fallback ??= valPattern;
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }

            return fallback;
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
            return null;
        }
    }

    private static InvokePattern? FindSelectFolderButton(AutomationElement dialog)
    {
        try
        {
            var buttons = dialog.FindAll(
                TreeScope.Descendants,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button));

            foreach (AutomationElement btn in buttons)
            {
                try
                {
                    var name = btn.Current.Name;
                    if (!string.IsNullOrEmpty(name) && name.Contains("Select Folder", StringComparison.OrdinalIgnoreCase))
                    {
                        if (btn.TryGetCurrentPattern(InvokePattern.Pattern, out var pat) && pat is InvokePattern invoke)
                            return invoke;
                    }
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return null;
    }

    /// <summary>Whether the extensions page lists FRIDAY's extension: its fixed id on the
    /// card ("ID: eiblop…"), never just the word FRIDAY (a title, a path).</summary>
    private static bool ShowsExtension(AutomationElement window)
    {
        try
        {
            foreach (AutomationElement e in window.FindAll(TreeScope.Descendants, Condition.TrueCondition))
            {
                try
                {
                    if ((e.Current.Name ?? "").Contains(BrowserLink.ExtensionId, StringComparison.Ordinal))
                        return true;
                }
                catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
                {
                }
            }
        }
        catch (Exception ex) when (ex is ElementNotAvailableException or InvalidOperationException or COMException)
        {
        }
        return false;
    }

    private static bool IsMatchingProcess(int pid, string targetProc)
    {
        try
        {
            using var p = Process.GetProcessById(pid);
            return string.Equals(p.ProcessName, targetProc, StringComparison.OrdinalIgnoreCase);
        }
        catch (Exception ex) when (ex is ArgumentException or InvalidOperationException)
        {
            return false;
        }
    }
}
