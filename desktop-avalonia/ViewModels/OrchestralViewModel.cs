using System;
using System.Collections.ObjectModel;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using MutexFlow.Desktop.Models;
using MutexFlow.Desktop.Services;

namespace MutexFlow.Desktop.ViewModels;

public partial class OrchestralViewModel : ViewModelBase
{
    private readonly IAgentClientService _client;

    [ObservableProperty] private string _runtimeStatusMessage = "Checking Orchestral runtime...";
    [ObservableProperty] private string _workspace = ".";
    [ObservableProperty] private string _jobStatusMessage = "No local analysis has been started.";
    [ObservableProperty] private bool _coreHealthy;
    [ObservableProperty] private bool _workerHealthy;
    [ObservableProperty] private bool _isBusy;

    public ObservableCollection<OrchestralApproval> PendingApprovals { get; } = new();
    public ObservableCollection<OrchestralAgent> Agents { get; } = new();
    public ObservableCollection<OrchestralFinding> Findings { get; } = new();
    public ObservableCollection<OrchestralAuditEntry> AuditEntries { get; } = new();

    public OrchestralViewModel(IAgentClientService client)
    {
        _client = client;
        _ = RefreshAsync();
    }

    [RelayCommand]
    public async Task RefreshAsync()
    {
        var statusTask = _client.GetOrchestralStatusAsync();
        var approvalsTask = _client.GetPendingApprovalsAsync();
        var agentsTask = _client.GetOrchestralAgentsAsync();
        var findingsTask = _client.GetOrchestralFindingsAsync();
        var auditTask = _client.GetOrchestralAuditAsync();

        JsonObject? status = await statusTask;
        CoreHealthy = status?["core_healthy"]?.GetValue<bool>() ?? false;
        WorkerHealthy = status?["worker_healthy"]?.GetValue<bool>() ?? false;
        RuntimeStatusMessage = status?["message"]?.GetValue<string>()
            ?? "MutexFlow backend is unavailable or Orchestral is not configured.";

        var approvals = await approvalsTask;
        foreach (var approval in approvals)
        {
            approval.ApproveCommand = new AsyncRelayCommand(() => ApproveAsync(approval));
            approval.DenyCommand = new AsyncRelayCommand(() => DenyAsync(approval));
        }
        Replace(PendingApprovals, approvals);
        Replace(Agents, await agentsTask);
        Replace(Findings, await findingsTask);
        Replace(AuditEntries, await auditTask);
    }

    [RelayCommand]
    public async Task StartLocalAnalysisAsync()
    {
        if (IsBusy) return;
        IsBusy = true;
        try
        {
            JobStatusMessage = "Submitting passive local workspace analysis...";
            var started = await _client.StartLocalAnalysisAsync(Workspace.Trim());
            if (!started.Ok || string.IsNullOrWhiteSpace(started.JobId))
            {
                JobStatusMessage = started.Error ?? "Could not start local analysis.";
                return;
            }

            JobStatusMessage = $"Analysis queued: {started.JobId}";
            for (var attempt = 0; attempt < 120; attempt++)
            {
                await Task.Delay(1000);
                var job = await _client.GetLocalAnalysisJobAsync(started.JobId);
                var status = job?["status"]?.GetValue<string>();
                if (status == "completed")
                {
                    var count = job?["findings_submitted"]?.GetValue<int>() ?? 0;
                    JobStatusMessage = $"Analysis complete. {count} masked finding(s) recorded in Citadel.";
                    await RefreshAsync();
                    return;
                }
                if (status == "failed")
                {
                    JobStatusMessage = job?["error"]?.GetValue<string>() ?? "Local analysis failed.";
                    return;
                }
                if (status is not null)
                    JobStatusMessage = $"Local analysis: {status}...";
            }

            JobStatusMessage = "Analysis is still running. Refresh the dashboard to check the latest status.";
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    public async Task ApproveAsync(OrchestralApproval? approval)
    {
        if (approval is null) return;
        var result = await _client.ResolveOrchestralApprovalAsync(approval.Id, approve: true);
        JobStatusMessage = result.Ok
            ? $"Approval granted for {approval.ToolName}."
            : $"Approval failed: {result.Error}";
        await RefreshAsync();
    }

    [RelayCommand]
    public async Task DenyAsync(OrchestralApproval? approval)
    {
        if (approval is null) return;
        var result = await _client.ResolveOrchestralApprovalAsync(approval.Id, approve: false);
        JobStatusMessage = result.Ok
            ? $"Request denied for {approval.ToolName}."
            : $"Denial failed: {result.Error}";
        await RefreshAsync();
    }

    private static void Replace<T>(ObservableCollection<T> collection, System.Collections.Generic.IEnumerable<T> items)
    {
        collection.Clear();
        foreach (var item in items)
            collection.Add(item);
    }
}
