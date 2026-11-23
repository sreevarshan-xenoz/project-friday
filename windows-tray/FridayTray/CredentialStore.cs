using System.Runtime.InteropServices;
using System.Text;

namespace FridayTray;

/// <summary>
/// Windows Credential Manager: FRIDAY's secrets as generic credentials named
/// `friday/NAME`, the value stored as UTF-16 text. Local to this machine and
/// this Windows user.
/// </summary>
internal static class CredentialStore
{
    private const uint TypeGeneric = 1;
    private const uint PersistLocalMachine = 2;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct Credential
    {
        public uint Flags;
        public uint Type;
        public string TargetName;
        public string? Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public string? TargetAlias;
        public string? UserName;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredReadW(string target, uint type, uint flags, out IntPtr credential);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredWriteW(ref Credential credential, uint flags);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredDeleteW(string target, uint type, uint flags);

    [DllImport("advapi32.dll")]
    private static extern void CredFree(IntPtr buffer);

    private static string Target(string name) => "friday/" + name;

    /// <summary>The stored value, or null when there is none.</summary>
    public static string? Read(string name)
    {
        if (!OperatingSystem.IsWindows() || !CredReadW(Target(name), TypeGeneric, 0, out var ptr))
            return null;
        try
        {
            var cred = Marshal.PtrToStructure<Credential>(ptr);
            return cred.CredentialBlob == IntPtr.Zero
                ? null
                : Marshal.PtrToStringUni(cred.CredentialBlob, (int)cred.CredentialBlobSize / 2);
        }
        finally
        {
            CredFree(ptr);
        }
    }

    /// <summary>Stores the value; false when Windows refuses.</summary>
    public static bool Write(string name, string value)
    {
        if (!OperatingSystem.IsWindows())
            return false;
        var blob = Encoding.Unicode.GetBytes(value);
        var handle = Marshal.AllocHGlobal(blob.Length);
        try
        {
            Marshal.Copy(blob, 0, handle, blob.Length);
            var cred = new Credential
            {
                Type = TypeGeneric,
                TargetName = Target(name),
                CredentialBlobSize = (uint)blob.Length,
                CredentialBlob = handle,
                Persist = PersistLocalMachine,
                UserName = Environment.UserName,
            };
            return CredWriteW(ref cred, 0);
        }
        finally
        {
            Array.Clear(blob);
            Marshal.FreeHGlobal(handle);
        }
    }

    /// <summary>Removes the value; nothing happens when there is none.</summary>
    public static void Delete(string name)
    {
        if (OperatingSystem.IsWindows())
            CredDeleteW(Target(name), TypeGeneric, 0);
    }
}
