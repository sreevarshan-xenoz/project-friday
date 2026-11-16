using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace WindowsHost.Win32;

/// <summary>
/// The default speakers' master volume and mute, through Core Audio
/// (IAudioEndpointVolume): exact levels, read back after they are set.
/// </summary>
[SupportedOSPlatform("windows")]
public static class CoreAudio
{
    [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
    private class MMDeviceEnumerator { }

    [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDeviceEnumerator
    {
        [PreserveSig] int EnumAudioEndpoints(int dataFlow, int stateMask, out IntPtr devices);
        [PreserveSig] int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice device);
    }

    [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDevice
    {
        [PreserveSig] int Activate(ref Guid iid, int clsCtx, IntPtr activationParams,
            [MarshalAs(UnmanagedType.IUnknown)] out object endpoint);
    }

    // The methods up to GetMute, in the order Windows declares them.
    [ComImport, Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IAudioEndpointVolume
    {
        [PreserveSig] int RegisterControlChangeNotify(IntPtr notify);
        [PreserveSig] int UnregisterControlChangeNotify(IntPtr notify);
        [PreserveSig] int GetChannelCount(out uint count);
        [PreserveSig] int SetMasterVolumeLevel(float db, ref Guid context);
        [PreserveSig] int SetMasterVolumeLevelScalar(float level, ref Guid context);
        [PreserveSig] int GetMasterVolumeLevel(out float db);
        [PreserveSig] int GetMasterVolumeLevelScalar(out float level);
        [PreserveSig] int SetChannelVolumeLevel(uint channel, float db, ref Guid context);
        [PreserveSig] int SetChannelVolumeLevelScalar(uint channel, float level, ref Guid context);
        [PreserveSig] int GetChannelVolumeLevel(uint channel, out float db);
        [PreserveSig] int GetChannelVolumeLevelScalar(uint channel, out float level);
        [PreserveSig] int SetMute([MarshalAs(UnmanagedType.Bool)] bool mute, ref Guid context);
        [PreserveSig] int GetMute([MarshalAs(UnmanagedType.Bool)] out bool mute);
    }

    private const int ERender = 0, EMultimedia = 1, ClsctxAll = 0x17;
    private static readonly Guid AudioEndpointVolumeId = new("5CDF2C82-841E-4546-9722-0CF74078229A");

    /// <summary>Thrown when there is nothing to set the volume of (no speakers).</summary>
    public sealed class NoOutputException() : Exception("There is no audio output device to control");

    private static IAudioEndpointVolume Endpoint()
    {
        var enumerator = (IMMDeviceEnumerator)new MMDeviceEnumerator();
        if (enumerator.GetDefaultAudioEndpoint(ERender, EMultimedia, out var device) < 0)
            throw new NoOutputException();
        var iid = AudioEndpointVolumeId;
        if (device.Activate(ref iid, ClsctxAll, IntPtr.Zero, out var endpoint) < 0)
            throw new NoOutputException();
        return (IAudioEndpointVolume)endpoint;
    }

    private static void Check(int hr, string what)
    {
        if (hr < 0)
            throw new InvalidOperationException($"Windows refused to {what} (0x{hr:X8})");
    }

    /// <summary>The master volume (0 to 100) and whether the speakers are muted.</summary>
    public static (int Level, bool Muted) Read()
    {
        var volume = Endpoint();
        Check(volume.GetMasterVolumeLevelScalar(out var level), "read the volume");
        Check(volume.GetMute(out var muted), "read the mute state");
        return ((int)Math.Round(level * 100), muted);
    }

    /// <summary>Sets the master volume (0 to 100) and returns what Windows now reports.</summary>
    public static (int Level, bool Muted) SetLevel(int percent)
    {
        var volume = Endpoint();
        var context = Guid.Empty;
        Check(volume.SetMasterVolumeLevelScalar(Math.Clamp(percent, 0, 100) / 100f, ref context), "set the volume");
        return Read();
    }

    /// <summary>Mutes, unmutes or (with null) flips the speakers; returns what Windows now reports.</summary>
    public static (int Level, bool Muted) SetMute(bool? mute)
    {
        var volume = Endpoint();
        Check(volume.GetMute(out var now), "read the mute state");
        var context = Guid.Empty;
        Check(volume.SetMute(mute ?? !now, ref context), "change the mute state");
        return Read();
    }
}
