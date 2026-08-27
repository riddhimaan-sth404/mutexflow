using System;
using System.Collections.ObjectModel;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using MutexFlow.Desktop.Models;
using MutexFlow.Desktop.Services;

namespace MutexFlow.Desktop.ViewModels;

public partial class LocalAiViewModel : ViewModelBase
{
    private readonly ILlamaProcessService _llamaService;
    private CancellationTokenSource? _downloadCts;

    public ObservableCollection<GgufModelItem> AvailableModels { get; } = new();

    [ObservableProperty]
    private GgufModelItem? _selectedModel;

    [ObservableProperty]
    private bool _isDownloading;

    [ObservableProperty]
    private double _downloadProgressValue;

    [ObservableProperty]
    private string _downloadStatusText = string.Empty;

    [ObservableProperty]
    private string _downloadButtonText = "Download & Setup";

    public LocalAiViewModel(ILlamaProcessService llamaService)
    {
        _llamaService = llamaService;
        InitializeModels();
    }

    private void InitializeModels()
    {
        AvailableModels.Add(new GgufModelItem
        {
            Name = "Llama 3.1 8B Instruct (Q4_K_M)",
            Repo = "lmstudio-community/Meta-Llama-3.1-8B-Instruct-GGUF",
            FileName = "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf"
        });
        AvailableModels.Add(new GgufModelItem
        {
            Name = "DeepSeek R1 Distill Llama 8B (Q4_K_M)",
            Repo = "unsloth/DeepSeek-R1-Distill-Llama-8B-GGUF",
            FileName = "DeepSeek-R1-Distill-Llama-8B-Q4_K_M.gguf"
        });
        AvailableModels.Add(new GgufModelItem
        {
            Name = "Qwen 2.5 Coder 7B Instruct (Q4_K_M)",
            Repo = "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
            FileName = "qwen2.5-coder-7b-instruct-q4_k_m.gguf"
        });
        AvailableModels.Add(new GgufModelItem
        {
            Name = "Phi-3.5 Mini Instruct (Q4_K_M)",
            Repo = "lmstudio-community/Phi-3.5-mini-instruct-GGUF",
            FileName = "Phi-3.5-mini-instruct-Q4_K_M.gguf"
        });
        AvailableModels.Add(new GgufModelItem
        {
            Name = "Mistral 7B Instruct v0.3 (Q4_K_M)",
            Repo = "maziyarpanahi/Mistral-7B-Instruct-v0.3-GGUF",
            FileName = "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf"
        });

        SelectedModel = AvailableModels[0];
    }

    [RelayCommand]
    public async Task DownloadModelAsync()
    {
        if (SelectedModel == null || IsDownloading)
            return;

        var destPath = Path.Combine(_llamaService.ModelsDirectory, SelectedModel.FileName);
        if (File.Exists(destPath))
        {
            DownloadStatusText = $"Model '{SelectedModel.FileName}' is already downloaded and ready.";
            DownloadProgressValue = 100;
            return;
        }

        IsDownloading = true;
        DownloadButtonText = "Downloading...";
        DownloadProgressValue = 0;
        DownloadStatusText = "Connecting to Hugging Face...";
        _downloadCts = new CancellationTokenSource();

        var progress = new Progress<(long Downloaded, long Total)>(p =>
        {
            if (p.Total > 0)
            {
                var pct = (double)p.Downloaded / p.Total * 100.0;
                DownloadProgressValue = pct;
                var mb = (p.Downloaded / 1024.0 / 1024.0).ToString("F1");
                var totalMb = (p.Total / 1024.0 / 1024.0).ToString("F1");
                DownloadStatusText = $"{mb} MB / {totalMb} MB ({pct:F1}%)";
            }
        });

        try
        {
            var success = await _llamaService.DownloadModelAsync(
                SelectedModel.Repo, SelectedModel.FileName, progress, _downloadCts.Token);

            if (success)
            {
                DownloadProgressValue = 100;
                DownloadStatusText = $"Download complete: {SelectedModel.FileName}";
            }
            else
            {
                DownloadStatusText = "Download failed. Please check your network connection.";
            }
        }
        catch (Exception ex)
        {
            DownloadStatusText = $"Download error: {ex.Message}";
        }
        finally
        {
            IsDownloading = false;
            DownloadButtonText = "Download & Setup";
        }
    }
}
