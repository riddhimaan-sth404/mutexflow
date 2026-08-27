using System;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace MutexFlow.Desktop.Services;

public interface ILlamaProcessService
{
    string ModelsDirectory { get; }
    string BinDirectory { get; }
    bool IsRunning { get; }
    Task<bool> EnsureLlamaRunningAsync(string modelFileName, CancellationToken ct = default);
    Task<bool> DownloadModelAsync(string repo, string file, IProgress<(long Downloaded, long Total)>? progress = null, CancellationToken ct = default);
    void StopLlama();
}

public class LlamaProcessService : ILlamaProcessService
{
    private Process? _llamaProcess;
    private readonly HttpClient _httpClient = new() { Timeout = TimeSpan.FromSeconds(3) };

    public string ModelsDirectory { get; }
    public string BinDirectory { get; }

    public bool IsRunning => _llamaProcess != null && !_llamaProcess.HasExited;

    public LlamaProcessService()
    {
        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        ModelsDirectory = Path.Combine(appData, "MutexFlow", "models");
        BinDirectory = Path.Combine(appData, "MutexFlow", "bin");

        Directory.CreateDirectory(ModelsDirectory);
        Directory.CreateDirectory(BinDirectory);
    }

    public string? FindLlamaBinary()
    {
        var binaryName = RuntimeInformation.IsOSPlatform(OSPlatform.Windows) ? "llama-server.exe" : "llama-server";
        var appBase = AppDomain.CurrentDomain.BaseDirectory;

        var candidates = new[]
        {
            Path.Combine(BinDirectory, binaryName),
            Path.Combine(appBase, "resources", "bin", binaryName),
            Path.Combine(appBase, binaryName),
            Path.Combine(appBase, "..", "..", "..", "resources", "bin", binaryName),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads", binaryName),
        };

        foreach (var path in candidates)
        {
            if (File.Exists(path))
                return Path.GetFullPath(path);
        }

        return null;
    }

    public async Task<bool> EnsureLlamaRunningAsync(string modelFileName, CancellationToken ct = default)
    {
        if (await CheckLlamaReadyAsync())
            return true;

        var binaryPath = FindLlamaBinary();
        if (binaryPath == null)
        {
            throw new FileNotFoundException(
                $"Local AI engine binary not found. Please place 'llama-server.exe' inside '{BinDirectory}' or the 'resources/bin' directory."
            );
        }

        var modelPath = Path.Combine(ModelsDirectory, modelFileName);
        if (!File.Exists(modelPath))
        {
            throw new FileNotFoundException(
                $"GGUF model file '{modelFileName}' not found. Please open Local AI Hub to download it first."
            );
        }

        var startInfo = new ProcessStartInfo
        {
            FileName = binaryPath,
            Arguments = $"--model \"{modelPath}\" --port 8080 --ctx-size 4096 --threads 4",
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };

        _llamaProcess = new Process { StartInfo = startInfo };
        _llamaProcess.Start();

        for (int i = 0; i < 30; i++)
        {
            if (ct.IsCancellationRequested)
            {
                StopLlama();
                return false;
            }

            if (await CheckLlamaReadyAsync())
                return true;

            await Task.Delay(1000, ct);
        }

        StopLlama();
        return false;
    }

    public async Task<bool> CheckLlamaReadyAsync()
    {
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            var resp = await _httpClient.GetAsync("http://localhost:8080/v1/models", cts.Token);
            return resp.IsSuccessStatusCode;
        }
        catch
        {
            return false;
        }
    }

    public async Task<bool> DownloadModelAsync(
        string repo,
        string file,
        IProgress<(long Downloaded, long Total)>? progress = null,
        CancellationToken ct = default)
    {
        var destPath = Path.Combine(ModelsDirectory, file);
        if (File.Exists(destPath))
            return true;

        var url = $"https://huggingface.co/{repo}/resolve/main/{file}";
        using var client = new HttpClient { Timeout = Timeout.InfiniteTimeSpan };
        using var response = await client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, ct);

        if (!response.IsSuccessStatusCode)
            return false;

        var totalBytes = response.Content.Headers.ContentLength ?? -1L;
        await using var contentStream = await response.Content.ReadAsStreamAsync(ct);
        await using var fileStream = new FileStream(destPath, FileMode.Create, FileAccess.Write, FileShare.None, 81920, true);

        var buffer = new byte[81920];
        long totalRead = 0;
        int read;

        while ((read = await contentStream.ReadAsync(buffer.AsMemory(0, buffer.Length), ct)) > 0)
        {
            await fileStream.WriteAsync(buffer.AsMemory(0, read), ct);
            totalRead += read;
            progress?.Report((totalRead, totalBytes));
        }

        return true;
    }

    public void StopLlama()
    {
        if (_llamaProcess != null)
        {
            try
            {
                if (!_llamaProcess.HasExited)
                    _llamaProcess.Kill(true);
            }
            catch { }
            finally
            {
                _llamaProcess.Dispose();
                _llamaProcess = null;
            }
        }
    }
}
