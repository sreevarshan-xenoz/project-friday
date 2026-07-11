namespace WindowsHost.Model;

/// <summary>
/// Window abstraction returned to the runtime. No raw HWND is ever exposed;
/// the host maps its internal handle to a stable Id.
/// </summary>
public class Bounds
{
    public int X { get; set; }
    public int Y { get; set; }
    public uint Width { get; set; }
    public uint Height { get; set; }
}

public class Window
{
    public string Id { get; set; } = "";
    public string Title { get; set; } = "";
    public string Process { get; set; } = "";
    public uint ProcessId { get; set; }
    public Bounds Bounds { get; set; } = new();
    public string State { get; set; } = "Normal"; // Normal | Minimized | Maximized | Hidden
    public uint Monitor { get; set; }
}
