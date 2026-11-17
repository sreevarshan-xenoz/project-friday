using System.Text;

namespace FridayTray.Core;

/// <summary>
/// The text of "Save a support report": versions and which switches are on, so
/// a person can say what is wrong without a terminal. Never questions, answers,
/// logs, keys or file contents.
/// </summary>
public static class SupportReport
{
    public static string Build(string version, string windows, string daemon, bool freeModels,
        bool desktopControl, bool screenText, string voice)
    {
        var b = new StringBuilder();
        b.AppendLine("FRIDAY support report (no questions, answers, logs or keys are included)");
        b.AppendLine($"FRIDAY: {version}");
        b.AppendLine($"Windows: {windows}");
        b.AppendLine($"FRIDAY says: {daemon}");
        b.AppendLine($"AI: {(freeModels ? "free models" : "own account or key")}");
        b.AppendLine($"Desktop control: {(desktopControl ? "on" : "off")}");
        b.AppendLine($"Read a window's text: {(screenText ? "on" : "off")}");
        b.AppendLine($"Voice: {voice}");
        return b.ToString();
    }
}
