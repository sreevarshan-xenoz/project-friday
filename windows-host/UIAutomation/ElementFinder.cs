namespace WindowsHost.UIAutomation;

/// <summary>
/// UI Automation element discovery. Real implementation will resolve controls
/// by automation id within a window for the future Vision module. Intentionally
/// minimal until Vision wiring lands — it returns a normalized code, never a
/// raw automation exception.
/// </summary>
public static class ElementFinder
{
    public static string Find(string windowId, string automationId)
    {
        // TODO: use System.Windows.Automation to locate the element.
        return "UIElementMissing";
    }
}
