using System.Diagnostics;
using System.Runtime.InteropServices;

namespace FridayTray.Core;

/// <summary>
/// An AI account tool FRIDAY can answer through (Claude Code, Codex, Antigravity, Cline)
/// using that tool's own CLI and sign-in.
/// </summary>
public sealed record AgentCli(
    string Id,
    string Label,
    string BinaryName,
    string InstallHint,
    string LoginCommand,
    IReadOnlyList<string> CheckArgsTemplate)
{
    public IReadOnlyList<string> CheckArgs(string prompt = AgentClis.DefaultCheckPrompt) =>
        CheckArgsTemplate.Select(a => a == "<q>" ? prompt : a).ToArray();
}

public static class AgentClis
{
    public const string DefaultCheckPrompt = "Reply with the single word: ready";
    public const string AgentClisVar = "FRIDAY_AGENT_CLIS";

    public static readonly IReadOnlyList<AgentCli> All =
    [
        new("claude",
            "Claude Code (your Anthropic account)",
            "claude",
            "irm https://claude.ai/install.ps1 | iex",
            "claude auth login",
            ["-p", "<q>", "--output-format", "text"]),

        new("codex",
            "Codex (your OpenAI / ChatGPT account)",
            "codex",
            "a native Windows build of Codex (codex.exe, from its GitHub releases); npm's codex.cmd can't be used by FRIDAY on Windows yet",
            "codex login",
            ["exec", "--skip-git-repo-check", "--sandbox", "read-only", "<q>"]),

        new("agy",
            "Antigravity (your Google account)",
            "agy",
            "install the Antigravity CLI (agy)",
            "agy",
            ["-p", "<q>", "--mode", "plan", "--effort", "low"]),

        new("cline",
            "Cline (your Cline account, incl. its free models)",
            "cline",
            "Cline installs as an npm .cmd shim, which FRIDAY can't use on Windows yet (it works on Linux)",
            "cline auth",
            ["-p", "--auto-approve", "false", "<q>"]),
    ];

    public static AgentCli? Find(string id) =>
        All.FirstOrDefault(c => string.Equals(c.Id, id.Trim(), StringComparison.OrdinalIgnoreCase));

    /// <summary>
    /// Searches for a binary on PATH, checking for .exe, .cmd, and .bat extensions.
    /// </summary>
    public static string? FindOnPath(string binaryName, string? path = null, string? pathext = null)
    {
        if (string.IsNullOrWhiteSpace(binaryName))
            return null;

        if (Path.IsPathRooted(binaryName) && File.Exists(binaryName))
            return binaryName;

        path ??= Environment.GetEnvironmentVariable("PATH");
        if (string.IsNullOrEmpty(path))
            return null;

        var exts = pathext?
            .Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(e => e.StartsWith('.') ? e : "." + e)
            .ToArray();

        exts ??= RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
            ? [".exe", ".cmd", ".bat"]
            : ["", ".exe", ".cmd", ".bat"];

        foreach (var rawDir in path.Split(Path.PathSeparator))
        {
            var dir = rawDir.Trim().Trim('"');
            if (dir.Length == 0 || !Directory.Exists(dir))
                continue;

            if (Path.HasExtension(binaryName))
            {
                var direct = Path.Combine(dir, binaryName);
                if (File.Exists(direct))
                    return direct;
            }

            foreach (var ext in exts)
            {
                var candidate = Path.Combine(dir, binaryName + ext);
                if (File.Exists(candidate))
                    return candidate;
            }

            var fallback = Path.Combine(dir, binaryName);
            if (File.Exists(fallback))
                return fallback;
        }

        return null;
    }

    /// <summary>
    /// The CLI's native executable. On Windows only an .exe counts: the daemon
    /// passes the question (newlines and all) as an argument, which Windows
    /// can't hand safely to an npm .cmd/.bat shim, so a shim-only install
    /// isn't usable by FRIDAY there yet.
    /// </summary>
    public static string? FindCli(AgentCli cli, string? path = null, string? pathext = null) =>
        FindOnPath(cli.BinaryName, path,
            pathext ?? (RuntimeInformation.IsOSPlatform(OSPlatform.Windows) ? ".exe" : null));

    public static bool IsInstalled(AgentCli cli, string? path = null, string? pathext = null) =>
        FindCli(cli, path, pathext) != null;

    /// <summary>
    /// Runs a headless check with the CLI in an empty temp working directory, using an argument array.
    /// Returns true if the process exits with code 0 and non-empty output within the timeout.
    /// </summary>
    public static async Task<bool> RunCheckAsync(
        AgentCli cli,
        string? binaryPath = null,
        string prompt = DefaultCheckPrompt,
        TimeSpan? timeout = null,
        CancellationToken cancel = default)
    {
        binaryPath ??= FindCli(cli);
        if (binaryPath == null)
            return false;

        var tempDir = Directory.CreateTempSubdirectory("friday-agent-check-");
        try
        {
            ProcessStartInfo psi;
            var isCmd = binaryPath.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase) ||
                        binaryPath.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);

            if (isCmd && RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            {
                var comspec = Environment.GetEnvironmentVariable("ComSpec") ?? "cmd.exe";
                psi = new ProcessStartInfo(comspec)
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    RedirectStandardInput = true,
                    WorkingDirectory = tempDir.FullName,
                };
                psi.ArgumentList.Add("/c");
                psi.ArgumentList.Add(binaryPath);
                foreach (var arg in cli.CheckArgs(prompt))
                    psi.ArgumentList.Add(arg);
            }
            else
            {
                psi = new ProcessStartInfo(binaryPath)
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    RedirectStandardInput = true,
                    WorkingDirectory = tempDir.FullName,
                };
                foreach (var arg in cli.CheckArgs(prompt))
                    psi.ArgumentList.Add(arg);
            }

            using var process = new Process { StartInfo = psi };
            try
            {
                process.Start();
            }
            catch (Exception)
            {
                return false;
            }

            try
            {
                process.StandardInput.Close();
            }
            catch { }

            var effectiveTimeout = timeout ?? TimeSpan.FromSeconds(120);
            using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancel);
            cts.CancelAfter(effectiveTimeout);

            var readStdout = process.StandardOutput.ReadToEndAsync();
            var readStderr = process.StandardError.ReadToEndAsync();

            try
            {
                await process.WaitForExitAsync(cts.Token);
            }
            catch (OperationCanceledException)
            {
                try { process.Kill(entireProcessTree: true); } catch { }
                return false;
            }

            var stdout = await readStdout;
            return process.ExitCode == 0 && !string.IsNullOrWhiteSpace(stdout);
        }
        finally
        {
            try { tempDir.Delete(recursive: true); } catch { }
        }
    }

    /// <summary>
    /// Parses the connected CLI ids from the FRIDAY_AGENT_CLIS line in env file text.
    /// </summary>
    public static IReadOnlyList<string> GetIds(string envText)
    {
        var ids = new List<string>();
        foreach (var raw in envText.Split('\n'))
        {
            var line = raw.TrimEnd('\r').Trim();
            if (line.Length == 0 || line.StartsWith('#'))
                continue;
            if (line.StartsWith(AgentClisVar + "=", StringComparison.Ordinal))
            {
                var val = line[(AgentClisVar.Length + 1)..].Trim();
                if (val.Length >= 2 && (val[0] == '"' || val[0] == '\'') && val[^1] == val[0])
                    val = val[1..^1];
                foreach (var item in val.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
                {
                    if (item.Length > 0 && !ids.Contains(item, StringComparer.OrdinalIgnoreCase))
                        ids.Add(item);
                }
            }
        }
        return ids;
    }

    public static bool HasId(string envText, string id) =>
        GetIds(envText).Contains(id.Trim(), StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Adds an agent CLI id to FRIDAY_AGENT_CLIS in env file text, deduplicated, keeping other lines.
    /// </summary>
    public static string AddId(string envText, string id)
    {
        id = id.Trim();
        if (id.Length == 0)
            return envText;

        var ids = GetIds(envText).ToList();
        if (!ids.Contains(id, StringComparer.OrdinalIgnoreCase))
            ids.Add(id);

        return SetAgentClis(envText, ids);
    }

    /// <summary>
    /// Removes an agent CLI id from FRIDAY_AGENT_CLIS in env file text, keeping other lines.
    /// </summary>
    public static string RemoveId(string envText, string id)
    {
        id = id.Trim();
        if (id.Length == 0)
            return envText;

        var ids = GetIds(envText).Where(x => !string.Equals(x, id, StringComparison.OrdinalIgnoreCase)).ToList();
        return SetAgentClis(envText, ids);
    }

    private static string SetAgentClis(string envText, IReadOnlyList<string> ids)
    {
        var lines = envText.Split('\n').Select(l => l.TrimEnd('\r')).ToList();
        if (lines.Count > 0 && lines[^1].Length == 0)
            lines.RemoveAt(lines.Count - 1);

        var firstIndex = -1;
        var resultLines = new List<string>();

        foreach (var line in lines)
        {
            var trimmed = line.Trim();
            if (trimmed.StartsWith(AgentClisVar + "=", StringComparison.Ordinal))
            {
                if (firstIndex == -1)
                {
                    firstIndex = resultLines.Count;
                    if (ids.Count > 0)
                        resultLines.Add($"{AgentClisVar}={string.Join(",", ids)}");
                }
            }
            else
            {
                resultLines.Add(line);
            }
        }

        if (firstIndex == -1 && ids.Count > 0)
        {
            resultLines.Add($"{AgentClisVar}={string.Join(",", ids)}");
        }

        if (resultLines.Count == 0)
            return "";

        return string.Join('\n', resultLines) + "\n";
    }
}
