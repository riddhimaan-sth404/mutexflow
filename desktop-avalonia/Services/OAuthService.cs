using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace MutexFlow.Desktop.Services;

public interface IOAuthService
{
    Task<bool> StartOAuthAsync(string provider, string machineId, string backendUrl, CancellationToken ct = default);
}

public class OAuthService : IOAuthService
{
    public async Task<bool> StartOAuthAsync(string provider, string machineId, string backendUrl, CancellationToken ct = default)
    {
        var authUrl = $"{backendUrl}/api/v1/auth/{provider}/connect?machineId={Uri.EscapeDataString(machineId)}";
        OpenBrowser(authUrl);

        var tempDir = Path.Combine(Path.GetTempPath(), "mutexflow-oauth");
        Directory.CreateDirectory(tempDir);
        var signalFile = Path.Combine(tempDir, $"{machineId}_{provider}");

        // Clean up any stale signal file
        if (File.Exists(signalFile))
        {
            try { File.Delete(signalFile); } catch { }
        }

        // Wait up to 120s for authentication callback completion
        for (int i = 0; i < 120; i++)
        {
            if (ct.IsCancellationRequested)
                return false;

            if (File.Exists(signalFile))
            {
                try { File.Delete(signalFile); } catch { }
                return true;
            }

            await Task.Delay(1000, ct);
        }

        return false;
    }

    private static void OpenBrowser(string url)
    {
        try
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            {
                Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
            }
            else if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
            {
                Process.Start("xdg-open", url);
            }
            else if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
            {
                Process.Start("open", url);
            }
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[OAuthService] Failed to open browser: {ex.Message}");
        }
    }
}
