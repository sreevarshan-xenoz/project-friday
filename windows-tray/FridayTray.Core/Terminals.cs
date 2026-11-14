namespace FridayTray.Core;

/// <summary>
/// Known terminal and console window classes. In these windows, synthetic Ctrl+C
/// interrupts running processes (SIGINT), so selection capture reads the clipboard
/// directly instead.
/// </summary>
public static class Terminals
{
    public const string ConsoleWindowClass = "ConsoleWindowClass";
    public const string WindowsTerminalClass = "CASCADIA_HOSTING_WINDOW_CLASS";
    public const string MinttyClass = "mintty";

    public static readonly string[] KnownClasses =
    [
        ConsoleWindowClass,
        WindowsTerminalClass,
        MinttyClass,
    ];

    public static bool IsTerminal(string? className) => IsTerminalClass(className);

    public static bool IsTerminalClass(string? className)
    {
        if (string.IsNullOrWhiteSpace(className))
            return false;

        var trimmed = className.Trim();
        foreach (var cls in KnownClasses)
        {
            if (string.Equals(trimmed, cls, StringComparison.OrdinalIgnoreCase))
                return true;
        }
        return false;
    }
}
