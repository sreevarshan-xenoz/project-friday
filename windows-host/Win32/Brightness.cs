using System.Management;
using System.Runtime.Versioning;

namespace WindowsHost.Win32;

/// <summary>
/// The built-in screen's brightness (a laptop panel), through WMI's
/// WmiMonitorBrightness classes. External monitors don't offer this to
/// Windows, so a desktop PC has nothing to set.
/// </summary>
[SupportedOSPlatform("windows")]
public static class Brightness
{
    /// <summary>Thrown when no screen can have its brightness set.</summary>
    public sealed class NoPanelException()
        : Exception("There is no built-in screen whose brightness Windows can set (it works on laptop screens)");

    // The query is lazy: WMI reports "Not supported" (no built-in screen) while
    // the results are enumerated, so they are collected inside the try.
    private static List<ManagementObject> Query(string cls)
    {
        try
        {
            var scope = new ManagementScope(@"\\.\root\wmi");
            scope.Connect();
            return new ManagementObjectSearcher(scope, new ObjectQuery($"SELECT * FROM {cls}"))
                .Get().Cast<ManagementObject>().ToList();
        }
        catch (ManagementException)
        {
            throw new NoPanelException();
        }
    }

    /// <summary>The brightness now (0 to 100).</summary>
    public static int Read()
    {
        foreach (var panel in Query("WmiMonitorBrightness"))
            return Convert.ToInt32(panel["CurrentBrightness"]);
        throw new NoPanelException();
    }

    /// <summary>Sets every built-in screen's brightness (1 to 100) and returns what Windows now reports.</summary>
    public static int Set(int percent)
    {
        var any = false;
        foreach (var panel in Query("WmiMonitorBrightnessMethods"))
        {
            panel.InvokeMethod("WmiSetBrightness", new object[] { 1u, (byte)Math.Clamp(percent, 1, 100) });
            any = true;
        }
        if (!any)
            throw new NoPanelException();
        return Read();
    }
}
