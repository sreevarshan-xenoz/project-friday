namespace WindowsHost.COM;

/// <summary>
/// Shell / COM interop module. Hosts only Windows-API interaction; no policy.
/// Expanded when composite operations need Shell.Application or other COM servers.
/// </summary>
public static class Shell
{
    public static string Run(string verb, string target) => "Unsupported";
}
