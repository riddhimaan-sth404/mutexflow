using System;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading.Tasks;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using MutexFlow.Desktop.Models;
using MutexFlow.Desktop.Services;

namespace MutexFlow.Desktop.ViewModels;

public partial class AgentViewModel : ViewModelBase
{
    private readonly IAgentClientService _agentClient;
    private readonly IHardwareFingerprintService _fingerprintService;
    private readonly ISecureStorageService _storage;
    private readonly ILlamaProcessService _llamaService;
    private readonly IntegrationsViewModel _integrationsVm;
    private readonly LocalAiViewModel _localAiVm;

    [ObservableProperty]
    private string _licenseKey = string.Empty;

    [ObservableProperty]
    private InferenceMode _selectedMode = InferenceMode.Local;

    [ObservableProperty]
    private string _byokKey = string.Empty;

    [ObservableProperty]
    private bool _isByokVisible;

    [ObservableProperty]
    private WorkflowType _selectedWorkflow = WorkflowType.GeneralReasoning;

    [ObservableProperty]
    private TaskComplexity _selectedComplexity = TaskComplexity.Medium;

    [ObservableProperty]
    private string _promptText = string.Empty;

    [ObservableProperty]
    private bool _isExecuting;

    [ObservableProperty]
    private bool _isCooldownActive;

    [ObservableProperty]
    private string _cooldownButtonText = "Run Local Agent";

    public ObservableCollection<LogEntry> Logs { get; } = new();

    public Array AvailableModes => Enum.GetValues(typeof(InferenceMode));
    public Array AvailableWorkflows => Enum.GetValues(typeof(WorkflowType));
    public Array AvailableComplexities => Enum.GetValues(typeof(TaskComplexity));

    public AgentViewModel(
        IAgentClientService agentClient,
        IHardwareFingerprintService fingerprintService,
        ISecureStorageService storage,
        ILlamaProcessService llamaService,
        IntegrationsViewModel integrationsVm,
        LocalAiViewModel localAiVm)
    {
        _agentClient = agentClient;
        _fingerprintService = fingerprintService;
        _storage = storage;
        _llamaService = llamaService;
        _integrationsVm = integrationsVm;
        _localAiVm = localAiVm;

        LicenseKey = _storage.GetSecret("license_key") ?? string.Empty;
        ByokKey = _storage.GetSecret("byok_key") ?? string.Empty;

        AddLog("MutexFlow initialized in Local-First AI mode.", LogType.System);
    }

    partial void OnSelectedModeChanged(InferenceMode value)
    {
        IsByokVisible = value == InferenceMode.Byok;
        CooldownButtonText = value switch
        {
            InferenceMode.Local => "Run Local Agent",
            InferenceMode.Standard => "Run Cloud Agent",
            InferenceMode.Byok => "Run BYOK Agent",
            _ => "Run Agent"
        };
    }

    partial void OnLicenseKeyChanged(string value)
    {
        if (!string.IsNullOrWhiteSpace(value))
            _storage.SaveSecret("license_key", value);
    }

    partial void OnByokKeyChanged(string value)
    {
        if (!string.IsNullOrWhiteSpace(value))
            _storage.SaveSecret("byok_key", value);
    }

    [RelayCommand]
    public void ClearLogs()
    {
        Logs.Clear();
    }

    [RelayCommand]
    public async Task ExecuteAgentAsync()
    {
        if (IsExecuting || IsCooldownActive)
            return;

        var prompt = PromptText.Trim();
        if (string.IsNullOrEmpty(prompt))
        {
            AddLog("Please enter a prompt for the AI assistant.", LogType.System);
            return;
        }

        var machineId = _fingerprintService.GetMachineId();
        var selectedIntegrations = _integrationsVm.Integrations
            .Where(i => i.IsSelected)
            .ToList();

        IsExecuting = true;

        try
        {
            // In Local mode, we prioritize offline local inference
            if (SelectedMode == InferenceMode.Local)
            {
                AddLog($"[Local AI] Processing workflow: {SelectedWorkflow}...", LogType.System);
                var (ok, res, err) = await _agentClient.RunLocalAsync(prompt, SelectedWorkflow, SelectedComplexity);

                if (!ok)
                {
                    AddLog("Local llama-server is not running. Attempting to start automatically...", LogType.System);
                    var selectedModel = _localAiVm.SelectedModel?.FileName;

                    if (string.IsNullOrEmpty(selectedModel))
                    {
                        AddLog("No GGUF model selected. Please open Local AI Hub to select and download a model.", LogType.Error);
                        return;
                    }

                    try
                    {
                        var started = await _llamaService.EnsureLlamaRunningAsync(selectedModel);
                        if (started)
                        {
                            AddLog("Local engine started on http://localhost:8080. Executing query...", LogType.System);
                            var retry = await _agentClient.RunLocalAsync(prompt, SelectedWorkflow, SelectedComplexity);
                            ok = retry.Ok;
                            res = retry.Result;
                            err = retry.Error;
                        }
                    }
                    catch (Exception ex)
                    {
                        err = ex.Message;
                    }
                }

                if (ok && res != null)
                {
                    AddLog("Output received:", LogType.Success);
                    AddLog(res, LogType.Success);

                    // Dispatch to user's connected integrations
                    foreach (var integ in selectedIntegrations)
                    {
                        AddLog($"Dispatching result to {integ.Name} with your credentials...", LogType.System);
                        var (dispOk, dispErr) = await _agentClient.DispatchDirectIntegrationAsync(
                            integ, res, prompt, SelectedWorkflow, SelectedComplexity);

                        if (dispOk)
                            AddLog($"Successfully delivered to {integ.Name}!", LogType.Success);
                        else
                            AddLog($"Delivery to {integ.Name} failed: {dispErr}", LogType.Error);
                    }
                }
                else
                {
                    AddLog($"Local AI Error: {err ?? "Unknown failure"}", LogType.Error);
                }
            }
            else if (SelectedMode == InferenceMode.Byok)
            {
                if (string.IsNullOrWhiteSpace(ByokKey))
                {
                    AddLog("OpenRouter API key required for BYOK mode.", LogType.Error);
                    return;
                }

                AddLog("Routing to OpenRouter via your API key...", LogType.System);
                var (ok, res, err) = await _agentClient.RunByokAsync(prompt, ByokKey.Trim(), SelectedWorkflow, SelectedComplexity);

                if (ok && res != null)
                {
                    AddLog("Output received:", LogType.Success);
                    AddLog(res, LogType.Success);
                }
                else
                {
                    AddLog($"BYOK Error: {err ?? "Unknown failure"}", LogType.Error);
                }
            }
            else
            {
                var license = LicenseKey.Trim();
                if (string.IsNullOrEmpty(license))
                {
                    AddLog("License key required for Cloud Standard mode.", LogType.Error);
                    return;
                }

                AddLog("Routing to Cloud failover cascade...", LogType.System);
                var (ok, res, err) = await _agentClient.RunStandardAsync(
                    prompt, license, machineId, SelectedWorkflow, SelectedComplexity,
                    selectedIntegrations.Select(i => i.Id).ToList());

                if (ok && res != null)
                {
                    AddLog("Output received:", LogType.Success);
                    AddLog(res, LogType.Success);
                }
                else
                {
                    AddLog($"Cloud Error: {err ?? "Unknown failure"}", LogType.Error);
                }
            }
        }
        catch (Exception ex)
        {
            AddLog($"Unexpected error: {ex.Message}", LogType.Error);
        }
        finally
        {
            IsExecuting = false;
            StartCooldown(GetCooldownSeconds(SelectedComplexity));
        }
    }

    private void AddLog(string text, LogType type)
    {
        Logs.Add(new LogEntry(text, type));
    }

    private void StartCooldown(int seconds)
    {
        IsCooldownActive = true;
        _ = Task.Run(async () =>
        {
            for (int r = seconds; r > 0; r--)
            {
                CooldownButtonText = $"Wait {r}s...";
                await Task.Delay(1000);
            }
            CooldownButtonText = SelectedMode switch
            {
                InferenceMode.Local => "Run Local Agent",
                InferenceMode.Standard => "Run Cloud Agent",
                _ => "Run Agent"
            };
            IsCooldownActive = false;
        });
    }

    private static int GetCooldownSeconds(TaskComplexity complexity) => complexity switch
    {
        TaskComplexity.Low => 1,
        TaskComplexity.Medium => 2,
        TaskComplexity.High => 5,
        TaskComplexity.Reasoning => 5,
        _ => 2
    };
}
