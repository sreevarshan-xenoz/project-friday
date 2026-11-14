namespace FridayTray.Core;

/// <summary>
/// Wait logic for foreground window restoration.
/// Polls until the foreground window handle matches the expected target handle,
/// or until the timeout elapses. The clock, delay, and getter are injectable for unit testing.
/// </summary>
public static class FocusWait
{
    public static readonly TimeSpan DefaultTimeout = TimeSpan.FromMilliseconds(500);
    public static readonly TimeSpan DefaultPollInterval = TimeSpan.FromMilliseconds(25);

    /// <summary>
    /// Asynchronously polls until <paramref name="getForeground"/> returns <paramref name="target"/>
    /// or <paramref name="timeout"/> expires.
    /// Returns true if foreground matched target within timeout; false if timed out or target is IntPtr.Zero.
    /// </summary>
    public static async Task<bool> WaitForForegroundAsync(
        IntPtr target,
        Func<IntPtr> getForeground,
        Func<TimeSpan, Task>? delay = null,
        Func<DateTime>? getNow = null,
        TimeSpan? timeout = null,
        TimeSpan? pollInterval = null)
    {
        if (target == IntPtr.Zero)
            return false;

        var timeoutVal = timeout ?? DefaultTimeout;
        var pollVal = pollInterval ?? DefaultPollInterval;
        if (pollVal <= TimeSpan.Zero)
            pollVal = DefaultPollInterval;
        var delayFn = delay ?? (t => Task.Delay(t));
        var clock = getNow ?? (() => DateTime.UtcNow);

        var deadline = clock() + timeoutVal;

        while (true)
        {
            if (getForeground() == target)
                return true;

            if (clock() >= deadline)
                return false;

            await delayFn(pollVal);
        }
    }

    /// <summary>
    /// Synchronously polls until <paramref name="getForeground"/> returns <paramref name="target"/>
    /// or <paramref name="timeout"/> expires.
    /// </summary>
    public static bool WaitForForeground(
        IntPtr target,
        Func<IntPtr> getForeground,
        Action<TimeSpan>? sleep = null,
        Func<DateTime>? getNow = null,
        TimeSpan? timeout = null,
        TimeSpan? pollInterval = null)
    {
        if (target == IntPtr.Zero)
            return false;

        var timeoutVal = timeout ?? DefaultTimeout;
        var pollVal = pollInterval ?? DefaultPollInterval;
        if (pollVal <= TimeSpan.Zero)
            pollVal = DefaultPollInterval;
        var sleepFn = sleep ?? (t => Thread.Sleep(t));
        var clock = getNow ?? (() => DateTime.UtcNow);

        var deadline = clock() + timeoutVal;

        while (true)
        {
            if (getForeground() == target)
                return true;

            if (clock() >= deadline)
                return false;

            sleepFn(pollVal);
        }
    }
}
