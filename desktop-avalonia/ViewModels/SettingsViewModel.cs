using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using MutexFlow.Desktop.Services;

namespace MutexFlow.Desktop.ViewModels;

public partial class SettingsViewModel : ViewModelBase
{
    private readonly IAgentClientService _agentClient;
    private readonly ISecureStorageService _storage;
    private readonly IHardwareFingerprintService _fingerprintService;

    [ObservableProperty]
    private string _backendUrl;

    [ObservableProperty]
    private string _machineId;

    [ObservableProperty]
    private string _statusMessage = string.Empty;

    public SettingsViewModel(
        IAgentClientService agentClient,
        ISecureStorageService storage,
        IHardwareFingerprintService fingerprintService)
    {
        _agentClient = agentClient;
        _storage = storage;
        _fingerprintService = fingerprintService;

        _backendUrl = _agentClient.BackendUrl;
        _machineId = _fingerprintService.GetMachineId();
    }

    [RelayCommand]
    public void SaveSettings()
    {
        _agentClient.BackendUrl = BackendUrl.Trim();
        _storage.SaveSecret("backend_url", BackendUrl.Trim());
        StatusMessage = "Settings saved successfully!";
    }
}
