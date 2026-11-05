namespace FridayTray.Core;

/// <summary>
/// When to bring a crashed child process back: after a growing delay (1s,
/// 2s, ...), and not at all once it has crashed more than five times inside a
/// minute. The same policy as `friday shell` on Linux.
/// </summary>
public sealed class RestartPolicy
{
    private readonly TimeSpan _window;
    private readonly int _maxRestarts;
    private DateTime _windowStart;
    private int _restarts;

    public RestartPolicy(int maxRestarts = 5, TimeSpan? window = null)
    {
        _maxRestarts = maxRestarts;
        _window = window ?? TimeSpan.FromMinutes(1);
    }

    /// <summary>
    /// Record a crash at <paramref name="now"/>. Returns the delay before the
    /// next start, or null to give up.
    /// </summary>
    public TimeSpan? OnCrash(DateTime now)
    {
        if (_restarts == 0 || now - _windowStart > _window)
        {
            _windowStart = now;
            _restarts = 0;
        }
        _restarts++;
        if (_restarts > _maxRestarts)
            return null;
        return TimeSpan.FromSeconds(_restarts);
    }

    public void Reset() => _restarts = 0;
}
