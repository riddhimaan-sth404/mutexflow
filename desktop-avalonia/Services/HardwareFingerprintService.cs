using System;
using System.IO;
using System.Runtime.Versioning;
using System.Security.Cryptography;
using Microsoft.Win32;

namespace MutexFlow.Desktop.Services;

public interface IHardwareFingerprintService
{
    string GetMachineId();
}

public class HardwareFingerprintService : IHardwareFingerprintService
{
    private string? _cachedId;

    public string GetMachineId()
    {
        if (!string.IsNullOrEmpty(_cachedId))
            return _cachedId;

        string rawId = string.Empty;

        if (OperatingSystem.IsWindows())
        {
            rawId = GetWindowsHardwareId();
        }
        else if (OperatingSystem.IsLinux())
        {
            rawId = GetLinuxMachineId();
        }
        else if (OperatingSystem.IsMacOS())
        {
            rawId = GetMacHardwareId();
        }

        if (string.IsNullOrWhiteSpace(rawId))
        {
            rawId = Environment.MachineName + "_" + Environment.UserName;
        }

        using var sha256 = SHA256.Create();
        byte[] hash = sha256.ComputeHash(System.Text.Encoding.UTF8.GetBytes(rawId));
        _cachedId = BitConverter.ToString(hash).Replace("-", "").ToLowerInvariant();

        return _cachedId;
    }

    [SupportedOSPlatform("windows")]
    private string GetWindowsHardwareId()
    {
        try
        {
            using var key = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Cryptography");
            if (key != null)
            {
                var guid = key.GetValue("MachineGuid")?.ToString();
                if (!string.IsNullOrWhiteSpace(guid))
                    return guid;
            }
        }
        catch { }

        return Environment.MachineName;
    }

    private string GetLinuxMachineId()
    {
        try
        {
            if (File.Exists("/etc/machine-id"))
                return File.ReadAllText("/etc/machine-id").Trim();
            if (File.Exists("/var/lib/dbus/machine-id"))
                return File.ReadAllText("/var/lib/dbus/machine-id").Trim();
        }
        catch { }

        return Environment.MachineName;
    }

    private string GetMacHardwareId()
    {
        return Environment.MachineName;
    }
}
