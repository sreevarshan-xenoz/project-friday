using System.Diagnostics;
using System.Runtime.InteropServices;
using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// Keeps one background program (the daemon or the Windows host) running:
/// started hidden, output appended to a log, brought back after a crash with
/// the same back-off as `friday shell` on Linux, and given up on (with an
/// event) after five crashes in a minute. All children share one job object,
/// so they end with the tray even if it crashes.
/// </summary>
internal sealed class Supervised : IDisposable
{
    private static readonly Lazy<IntPtr> Job = new(CreateKillOnCloseJob);

    private readonly string _name;
    private readonly string _exe;
    private readonly Func<IDictionary<string, string>> _env;
    private readonly string _logPath;
    private readonly RestartPolicy _policy = new();
    private readonly object _lock = new();
    private Process? _process;
    private StreamWriter? _log;
    private bool _stopping;

    public Supervised(string name, string exe, Func<IDictionary<string, string>> env, string logDir)
    {
        _name = name;
        _exe = exe;
        _env = env;
        _logPath = Path.Combine(logDir, name + ".log");
    }

    public event Action<string>? GaveUp;

    public bool Running { get { lock (_lock) return _process is { HasExited: false }; } }

    public void Start()
    {
        lock (_lock)
        {
            _stopping = false;
            StartLocked();
        }
    }

    // Callers hold _lock.
    private void StartLocked()
    {
        if (_process is { HasExited: false })
            return;
        OpenLog();
        var psi = new ProcessStartInfo(_exe)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = Path.GetDirectoryName(_exe)!,
        };
        foreach (var (k, v) in _env())
            psi.Environment[k] = v;
        var p = new Process { StartInfo = psi, EnableRaisingEvents = true };
        p.OutputDataReceived += (_, e) => Write(e.Data);
        p.ErrorDataReceived += (_, e) => Write(e.Data);
        p.Exited += (_, _) => OnExited(p);
        p.Start();
        if (Job.Value != IntPtr.Zero)
            Native.AssignProcessToJobObject(Job.Value, p.Handle);
        p.BeginOutputReadLine();
        p.BeginErrorReadLine();
        _process = p;
        Write($"--- {_name} started (pid {p.Id}) {DateTime.Now:yyyy-MM-dd HH:mm:ss}");
    }

    /// <summary>Stop on purpose (quit, restart, new AI key): no restart follows.</summary>
    public void Stop()
    {
        Process? p;
        lock (_lock)
        {
            _stopping = true;
            p = _process;
            _process = null;
        }
        if (p is { HasExited: false })
        {
            try { p.Kill(entireProcessTree: true); p.WaitForExit(5000); } catch (InvalidOperationException) { }
        }
    }

    public void Restart()
    {
        Stop();
        _policy.Reset();
        Start();
    }

    private void OnExited(Process p)
    {
        TimeSpan? delay;
        lock (_lock)
        {
            if (_stopping || !ReferenceEquals(p, _process))
                return;
            Write($"--- {_name} exited with code {p.ExitCode} {DateTime.Now:yyyy-MM-dd HH:mm:ss}");
            _process = null;
            delay = _policy.OnCrash(DateTime.Now);
        }
        if (delay is null)
        {
            Write($"--- {_name} keeps crashing; not restarting");
            GaveUp?.Invoke(_name);
            return;
        }
        // Check and start under one lock: a Stop() (quit, new AI key) that
        // lands while the restart waits must win, and Start() would clear it.
        Task.Delay(delay.Value).ContinueWith(_ =>
        {
            lock (_lock)
            {
                if (_stopping)
                    return;
                try { StartLocked(); } catch (Exception e) { Write($"--- restart failed: {e.Message}"); }
            }
        });
    }

    private void OpenLog()
    {
        if (_log != null)
            return;
        Directory.CreateDirectory(Path.GetDirectoryName(_logPath)!);
        // Keep one previous log once it passes 5 MB.
        if (File.Exists(_logPath) && new FileInfo(_logPath).Length > 5 * 1024 * 1024)
            File.Move(_logPath, _logPath + ".1", overwrite: true);
        _log = new StreamWriter(new FileStream(_logPath, FileMode.Append, FileAccess.Write, FileShare.ReadWrite)) { AutoFlush = true };
    }

    private void Write(string? line)
    {
        if (line == null)
            return;
        lock (_lock)
            _log?.WriteLine(line);
    }

    public void Dispose()
    {
        Stop();
        lock (_lock)
        {
            _log?.Dispose();
            _log = null;
        }
    }

    private static IntPtr CreateKillOnCloseJob()
    {
        var job = Native.CreateJobObjectW(IntPtr.Zero, null);
        if (job == IntPtr.Zero)
            return IntPtr.Zero;
        var info = new Native.JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
        info.BasicLimitInformation.LimitFlags = Native.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        Native.SetInformationJobObject(job, Native.JobObjectExtendedLimitInformation, ref info,
            Marshal.SizeOf<Native.JOBOBJECT_EXTENDED_LIMIT_INFORMATION>());
        return job;
    }
}
