using System.Runtime.Versioning;
using Microsoft.Win32;

namespace WindowsHost.Win32;

/// <summary>
/// Turns what a person calls an app ("brave", "chrome.exe", "microsoft teams")
/// into something to start. A program on PATH starts as it is; otherwise the
/// places Windows itself looks when you type a name: the App Paths registry
/// (where installers register `brave.exe`, `chrome.exe`...), then the Start
/// menu's shortcuts, best name match first.
/// </summary>
[SupportedOSPlatform("windows")]
public static class AppResolver
{
    /// <summary>What to start: a program with its arguments, or a shortcut left to the shell.</summary>
    public sealed record Target(string FileName, string Arguments, bool Shell, string Via);

    private static readonly string[] Noise = ["uninstall", "readme", "help", "documentation", "release notes", "license"];

    public static Target? Resolve(string name)
    {
        name = name.Trim().Trim('"');
        if (name.Length == 0)
            return null;
        // An explicit path is the caller's business.
        if (name.Contains('\\') || name.Contains('/') || Path.IsPathRooted(name))
            return new Target(name, "", false, "path");

        var exe = name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) ? name : name + ".exe";
        var onPath = OnPath(exe);
        if (onPath is not null)
            return new Target(onPath, "", false, "PATH");
        var registered = AppPaths(exe);
        if (registered is not null)
            return new Target(registered, "", false, "App Paths");
        return StartMenu(Path.GetFileNameWithoutExtension(exe));
    }

    private static string? OnPath(string exe)
    {
        foreach (var dir in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(';', StringSplitOptions.RemoveEmptyEntries))
        {
            try
            {
                var candidate = Path.Combine(dir.Trim(), exe);
                if (File.Exists(candidate))
                    return candidate;
            }
            catch (ArgumentException)
            {
                // A malformed PATH entry is skipped.
            }
        }
        return null;
    }

    private static string? AppPaths(string exe)
    {
        foreach (var (hive, view) in new[]
        {
            (RegistryHive.CurrentUser, RegistryView.Default),
            (RegistryHive.LocalMachine, RegistryView.Registry64),
            (RegistryHive.LocalMachine, RegistryView.Registry32),
        })
        {
            using var root = RegistryKey.OpenBaseKey(hive, view);
            using var key = root.OpenSubKey($@"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
            var value = (key?.GetValue(null) as string)?.Trim().Trim('"');
            if (!string.IsNullOrEmpty(value) && File.Exists(value))
                return value;
        }
        return null;
    }

    /// <summary>The Start menu shortcuts that best match <paramref name="query"/> (best first, at most <paramref name="limit"/>).</summary>
    public static List<string> Search(string query, int limit)
    {
        var words = query.ToLowerInvariant()
            .Split([' ', '-', '_', '.'], StringSplitOptions.RemoveEmptyEntries)
            .Where(w => w.Length >= 2 && w is not ("an" or "the" or "app" or "another" or "new" or "my"))
            .ToArray();
        var scored = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);
        foreach (var file in StartMenuShortcuts())
        {
            var name = Path.GetFileNameWithoutExtension(file);
            if (Noise.Any(n => name.Contains(n, StringComparison.OrdinalIgnoreCase)))
                continue;
            var total = words.Sum(w => Score(name, w));
            if (total > 0 && (!scored.TryGetValue(name, out var best) || total > best))
                scored[name] = total;
        }
        return scored.OrderByDescending(kv => kv.Value).ThenBy(kv => kv.Key).Take(limit).Select(kv => kv.Key).ToList();
    }

    private static IEnumerable<string> StartMenuShortcuts()
    {
        foreach (var root in new[]
        {
            Environment.GetFolderPath(Environment.SpecialFolder.CommonPrograms),
            Environment.GetFolderPath(Environment.SpecialFolder.Programs),
        })
        {
            if (root.Length == 0 || !Directory.Exists(root))
                continue;
            IEnumerable<string> files;
            try
            {
                files = Directory.EnumerateFiles(root, "*.lnk", SearchOption.AllDirectories).ToList();
            }
            catch (Exception e) when (e is UnauthorizedAccessException or IOException)
            {
                continue;
            }
            foreach (var f in files)
                yield return f;
        }
    }

    private static Target? StartMenu(string name)
    {
        var roots = new[]
        {
            Environment.GetFolderPath(Environment.SpecialFolder.CommonPrograms),
            Environment.GetFolderPath(Environment.SpecialFolder.Programs),
        };
        string? best = null;
        var bestScore = 0;
        foreach (var root in roots.Where(r => r.Length > 0 && Directory.Exists(r)))
        {
            IEnumerable<string> files;
            try
            {
                files = Directory.EnumerateFiles(root, "*.lnk", SearchOption.AllDirectories);
            }
            catch (Exception e) when (e is UnauthorizedAccessException or IOException)
            {
                continue;
            }
            foreach (var file in files)
            {
                var score = Score(Path.GetFileNameWithoutExtension(file), name);
                if (score > bestScore)
                {
                    best = file;
                    bestScore = score;
                }
            }
        }
        return best is null ? null : FromShortcut(best);
    }

    /// <summary>3 exact, 2 starts with, 1 contains, or the name is the shortcut and more
    /// words ("brave browser" for Brave, "google chrome" for Chrome); 0 no match. A sentence that only mentions an
    /// app ("youtube in brave") is no match: it would start that app with nothing in it.</summary>
    public static int Score(string shortcut, string name)
    {
        var s = shortcut.Trim().ToLowerInvariant();
        var n = name.Trim().ToLowerInvariant();
        if (n.Length == 0 || Noise.Any(s.Contains))
            return 0;
        if (s == n) return 3;
        if (s.StartsWith(n, StringComparison.Ordinal)) return 2;
        if (s.Contains(n, StringComparison.Ordinal) || n.StartsWith(s + " ", StringComparison.Ordinal)) return 1;
        // "google chrome" for Chrome: a two-word name ending in the shortcut's.
        if (n.EndsWith(" " + s, StringComparison.Ordinal) && n.Count(c => c == ' ') == 1) return 1;
        return 0;
    }

    private static Target FromShortcut(string lnk)
    {
        try
        {
            var shell = Activator.CreateInstance(Type.GetTypeFromProgID("WScript.Shell")!)!;
            dynamic link = ((dynamic)shell).CreateShortcut(lnk);
            string target = link.TargetPath;
            string args = link.Arguments;
            if (target.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) && File.Exists(target))
                return new Target(target, args ?? "", false, "Start menu");
        }
        catch (Exception e) when (e is System.Runtime.InteropServices.COMException or InvalidCastException
            or Microsoft.CSharp.RuntimeBinder.RuntimeBinderException or NullReferenceException)
        {
            // Not a program shortcut we can read: let the shell open it.
        }
        return new Target(lnk, "", true, "Start menu");
    }
}
