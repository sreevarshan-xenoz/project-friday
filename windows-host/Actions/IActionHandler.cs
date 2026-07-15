using System.Text.Json;
using WindowsHost.IPC;
using WindowsHost.Win32;

namespace WindowsHost.Actions;

/// <summary>
/// One handler per intent. Handlers contain ONLY Windows API interaction — no
/// planner, memory, AI, or permission logic. They return a normalized
/// <see cref="BridgeResponse"/> (error codes never leak HRESULT/Win32 values).
/// </summary>
public interface IActionHandler
{
    /// <summary>The intent this handler serves, e.g. "windows.open".</summary>
    string Intent { get; }

    BridgeResponse Handle(JsonElement payload, WindowRegistry registry);
}
