namespace FridayTray.Core;

/// <summary>
/// The first-run welcome: shown once, on the first launch, and again from the
/// tray menu whenever the person wants it.
/// </summary>
public static class Welcome
{
    private static string Marker(Paths paths) => Path.Combine(paths.StateDir, "welcomed");

    public static bool Pending(Paths paths) => !File.Exists(Marker(paths));

    public static void MarkShown(Paths paths)
    {
        Directory.CreateDirectory(paths.StateDir);
        File.WriteAllText(Marker(paths), "");
    }
}
