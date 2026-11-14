namespace FridayTray.Core;

/// <summary>
/// Decision and retry logic for registering system-wide hotkeys.
/// Retries the primary choice before falling back to secondary choices,
/// preventing transient registration failures (e.g. during a rapid restart
/// when the previous process hasn't fully released the hotkey) from latching
/// the fallback hotkey.
/// </summary>
public static class HotkeyRegistration
{
    public const int DefaultPrimaryAttempts = 3;
    public static readonly TimeSpan DefaultRetryDelay = TimeSpan.FromMilliseconds(500);

    /// <summary>
    /// Attempts to register a hotkey from a list of choices (primary first, then fallbacks).
    /// The primary choice is tried up to <paramref name="primaryAttempts"/> times, waiting
    /// <paramref name="retryDelay"/> between attempts. If the primary choice fails all attempts,
    /// each subsequent fallback is tried once without delay.
    /// </summary>
    /// <typeparam name="T">The hotkey candidate representation.</typeparam>
    /// <param name="choices">Ordered list of hotkey choices (primary first).</param>
    /// <param name="tryRegister">Function attempting registration; returns true on success.</param>
    /// <param name="primaryAttempts">Number of attempts for the primary choice (default 3).</param>
    /// <param name="retryDelay">Delay between primary retry attempts (default 500ms).</param>
    /// <param name="delay">Injectable delay action (defaults to Thread.Sleep).</param>
    /// <returns>A tuple (Success, Chosen) indicating if registration succeeded and which choice won.</returns>
    public static (bool Success, T Chosen) RegisterWithRetry<T>(
        IReadOnlyList<T> choices,
        Func<T, bool> tryRegister,
        int primaryAttempts = DefaultPrimaryAttempts,
        TimeSpan? retryDelay = null,
        Action<TimeSpan>? delay = null)
    {
        if (choices == null || choices.Count == 0)
            return (false, default!);

        var delayAction = delay ?? (t => Thread.Sleep(t));
        var wait = retryDelay ?? DefaultRetryDelay;

        // 1. Try primary choice with retries
        var primary = choices[0];
        for (var attempt = 1; attempt <= primaryAttempts; attempt++)
        {
            if (tryRegister(primary))
                return (true, primary);

            if (attempt < primaryAttempts && wait > TimeSpan.Zero)
                delayAction(wait);
        }

        // 2. Try fallback choices in order
        for (var i = 1; i < choices.Count; i++)
        {
            var fallback = choices[i];
            if (tryRegister(fallback))
                return (true, fallback);
        }

        return (false, default!);
    }
}
