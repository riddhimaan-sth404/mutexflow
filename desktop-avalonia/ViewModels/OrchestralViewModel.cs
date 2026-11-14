using System;
using System.Collections.ObjectModel;
using System.Linq;
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
    [ObservableProperty] private string _activeJobId = string.Empty;
    [ObservableProperty] private string _jcodeStatusMessage = "Checking JCode runtime...";
    [ObservableProperty] private string _jcodeWorkspace = ".";
    [ObservableProperty] private string _jcodeRole = "workspace_analyst";
    [ObservableProperty] private string _jcodeObjective = "Review this workspace and report evidence-backed engineering observations.";
    private long _lastJcodeEventSequence;
    [ObservableProperty] private string _jcodePrompt = string.Empty;
    [ObservableProperty] private string _jcodeSessionId = string.Empty;
    [ObservableProperty] private string _jcodeResult = string.Empty;
    [ObservableProperty] private string _jcodeBatchTasks = string.Empty;
    [ObservableProperty] private string _jcodeBatchId = string.Empty;
    [ObservableProperty] private bool _jcodeConnected;
    [ObservableProperty] private bool _jcodeProviderReady;
    [ObservableProperty] private bool _isJcodeBusy;

    public ObservableCollection<OrchestralApproval> PendingApprovals { get; } = new();
    public ObservableCollection<OrchestralAgent> Agents { get; } = new();
    public ObservableCollection<OrchestralFinding> Findings { get; } = new();
    public ObservableCollection<OrchestralAuditEntry> AuditEntries { get; } = new();
    public ObservableCollection<string> JcodeSessions { get; } = new();
    public ObservableCollection<string> JcodeRoles { get; } = new() { "workspace_analyst", "security_reviewer", "dependency_reviewer", "documentation_reviewer" };

    public OrchestralViewModel(IAgentClientService client)
    {
        _client = client;
        _ = RefreshAsync();
    }

    [RelayCommand]
    public async Task RefreshAsync()
    {
        var statusTask = _client.GetOrchestralStatusAsync();
        var jcodeStatusTask = _client.GetJcodeStatusAsync();
        var jcodeSessionsTask = _client.GetJcodeSessionsAsync();
        var approvalsTask = _client.GetPendingApprovalsAsync();
        var agentsTask = _client.GetOrchestralAgentsAsync();
        var findingsTask = _client.GetOrchestralFindingsAsync();
        var auditTask = _client.GetOrchestralAuditAsync();

        JsonObject? status = await statusTask;
        CoreHealthy = status?["core_healthy"]?.GetValue<bool>() ?? false;
        WorkerHealthy = status?["worker_healthy"]?.GetValue<bool>() ?? false;
        RuntimeStatusMessage = status?["message"]?.GetValue<string>()
            ?? "MutexFlow backend is unavailable or Orchestral is not configured.";
        var jcodeStatus = await jcodeStatusTask;
        JcodeConnected = jcodeStatus?["connected"]?.GetValue<bool>() ?? false;
        JcodeProviderReady = jcodeStatus?["provider_ready"]?.GetValue<bool>() ?? false;
        JcodeStatusMessage = jcodeStatus?["message"]?.GetValue<string>()
            ?? "JCode status unavailable. Check the MutexFlow backend.";
        var knownJcodeSessions = await jcodeSessionsTask;
        JcodeSessions.Clear();
        foreach (var sessionId in knownJcodeSessions) JcodeSessions.Add(sessionId);
        if (string.IsNullOrWhiteSpace(JcodeSessionId) && JcodeSessions.Count > 0)
            JcodeSessionId = JcodeSessions[0];

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

            ActiveJobId = started.JobId;
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
                    ActiveJobId = string.Empty;
                    await RefreshAsync();
                    return;
                }
                if (status == "failed")
                {
                    JobStatusMessage = job?["error"]?.GetValue<string>() ?? "Local analysis failed.";
                    ActiveJobId = string.Empty;
                    return;
                }
                if (status == "cancelled")
                {
                    JobStatusMessage = "Local analysis cancelled. Any findings already discovered were retained.";
                    ActiveJobId = string.Empty;
                    await RefreshAsync();
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
    public async Task CancelLocalAnalysisAsync()
    {
        if (string.IsNullOrWhiteSpace(ActiveJobId)) return;
        var result = await _client.CancelLocalAnalysisAsync(ActiveJobId);
        JobStatusMessage = result.Ok
            ? "Cancellation requested. Waiting for the worker to stop..."
            : $"Cancellation failed: {result.Error}";
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


    [RelayCommand]
    public async Task CreateJcodeSessionAsync()
    {
        if (!JcodeConnected || !JcodeProviderReady || IsJcodeBusy) return;
        IsJcodeBusy = true;
        try
        {
            if (!CoreHealthy)
            {
                JcodeStatusMessage = "Citadel must be healthy before creating an Orchestral-managed JCode agent.";
                return;
            }
            var created = await _client.CreateJcodeSessionAsync(JcodeWorkspace.Trim(), JcodeRole, JcodeObjective.Trim());
            if (!created.Ok || string.IsNullOrWhiteSpace(created.SessionId))
            {
                JcodeStatusMessage = created.Error ?? "Could not create JCode session.";
                return;
            }
            JcodeSessionId = created.SessionId;
            if (!JcodeSessions.Contains(created.SessionId))
                JcodeSessions.Add(created.SessionId);
            JcodeStatusMessage = $"Created isolated JCode agent session {created.SessionId}.";
        }
        finally { IsJcodeBusy = false; }
    }

    [RelayCommand]
    public async Task RunJcodeSessionAsync()
    {
        if (!JcodeProviderReady || string.IsNullOrWhiteSpace(JcodeSessionId) || string.IsNullOrWhiteSpace(JcodePrompt) || IsJcodeBusy) return;
        IsJcodeBusy = true;
        try
        {
            JcodeStatusMessage = "JCode agent is processing the task...";
            JcodeResult = string.Empty;
            var previousEvents = await _client.GetJcodeEventsAsync(0);
            _lastJcodeEventSequence = previousEvents.Count == 0 ? 0 : previousEvents.Max(item => item["sequence"]?.GetValue<long>() ?? 0);
            var runTask = _client.RunJcodeSessionAsync(JcodeSessionId, JcodePrompt);
            while (!runTask.IsCompleted)
            {
                await RefreshJcodeEventsAsync(true);
                await Task.Delay(150);
            }
            var result = await runTask;
            await RefreshJcodeEventsAsync(true);
            if (!result.Ok)
            {
                JcodeStatusMessage = result.Error ?? "JCode turn failed.";
                return;
            }
            JcodeResult = result.Result ?? string.Empty;
            JcodeStatusMessage = "JCode turn completed.";
        }
        finally { IsJcodeBusy = false; }
    }


    [RelayCommand]
    public async Task RunJcodeBatchAsync()
    {
        if (!JcodeConnected || IsJcodeBusy) return;
        var tasks = JcodeBatchTasks.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (tasks.Length == 0 || tasks.Length > 8)
        {
            JcodeStatusMessage = "Enter between 1 and 8 tasks, one per line.";
            return;
        }
        IsJcodeBusy = true;
        try
        {
            if (!CoreHealthy)
            {
                JcodeStatusMessage = "Citadel must be healthy before starting an Orchestral agent batch.";
                return;
            }
            JcodeResult = string.Empty;
            var previousEvents = await _client.GetJcodeEventsAsync(0);
            _lastJcodeEventSequence = previousEvents.Count == 0 ? 0 : previousEvents.Max(item => item["sequence"]?.GetValue<long>() ?? 0);
            JcodeStatusMessage = $"Starting {tasks.Length} concurrent JCode agent sessions...";
            var started = await _client.StartJcodeBatchAsync(JcodeWorkspace.Trim(), tasks.ToList());
            if (!started.Ok || started.Batch is null)
            {
                JcodeStatusMessage = started.Error ?? "Could not start the JCode agent batch.";
                return;
            }
            JcodeBatchId = started.Batch["batch_id"]?.GetValue<string>() ?? string.Empty;
            if (started.Batch["session_ids"] is JsonArray sessionIds)
            {
                foreach (var item in sessionIds)
                {
                    var id = item?.GetValue<string>();
                    if (string.IsNullOrWhiteSpace(id)) continue;
                    if (!JcodeSessions.Contains(id)) JcodeSessions.Add(id);
                    if (string.IsNullOrWhiteSpace(JcodeSessionId)) JcodeSessionId = id;
                }
            }
            JsonObject? batchStatus = null;
            while (!string.IsNullOrWhiteSpace(JcodeBatchId))
            {
                await RefreshJcodeEventsAsync();
                batchStatus = await _client.GetJcodeBatchAsync(JcodeBatchId);
                var state = batchStatus?["status"]?.GetValue<string>();
                if (state is "completed" or "completed_with_errors" or "failed" or "cancelled")
                    break;
                if (batchStatus is null)
                {
                    JcodeStatusMessage = "Could not retrieve JCode batch status.";
                    break;
                }
                await Task.Delay(300);
            }
            await RefreshJcodeEventsAsync();
            var finalState = batchStatus?["status"]?.GetValue<string>() ?? "unknown";
            JcodeStatusMessage = $"Concurrent JCode batch {finalState}. Batch ID: {JcodeBatchId}";
            if (batchStatus?["results"] is JsonArray results)
                JcodeResult += Environment.NewLine + Environment.NewLine + "Batch results:" + Environment.NewLine + results.ToJsonString();
        }
        finally { IsJcodeBusy = false; }
    }

    [RelayCommand]
    public async Task CancelJcodeBatchAsync()
    {
        if (string.IsNullOrWhiteSpace(JcodeBatchId)) return;
        var result = await _client.CancelJcodeBatchAsync(JcodeBatchId);
        JcodeStatusMessage = result.Ok
            ? $"Cancellation requested for batch {JcodeBatchId}."
            : $"Batch cancellation failed: {result.Error}";
    }

    private async Task RefreshJcodeEventsAsync(bool onlySelectedSession = false)
    {
        var events = await _client.GetJcodeEventsAsync(_lastJcodeEventSequence);
        foreach (var item in events)
        {
            var sequence = item["sequence"]?.GetValue<long>() ?? 0;
            if (sequence > _lastJcodeEventSequence) _lastJcodeEventSequence = sequence;
            if (onlySelectedSession && item["session_id"]?.GetValue<string>() != JcodeSessionId) continue;
            var kind = item["kind"]?.GetValue<string>() ?? string.Empty;
            var text = item["text"]?.GetValue<string>();
            switch (kind)
            {
                case "text_delta":
                    if (!string.IsNullOrEmpty(text)) JcodeResult += text;
                    break;
                case "text_replace":
                    JcodeResult = text ?? string.Empty;
                    break;
                case "tool_call":
                    var tool = item["tool_name"]?.GetValue<string>() ?? "workspace tool";
                    JcodeStatusMessage = $"JCode is using the Citadel-audited read-only {tool} tool...";
                    break;
                case "error":
                    JcodeStatusMessage = text ?? "JCode reported a runtime error.";
                    break;
            }
        }
    }

    [RelayCommand]
    public async Task CancelJcodeSessionAsync()
    {
        if (string.IsNullOrWhiteSpace(JcodeSessionId)) return;
        var result = await _client.CancelJcodeSessionAsync(JcodeSessionId);
        JcodeStatusMessage = result.Ok
            ? "Cancellation requested for the selected JCode session."
            : $"JCode cancellation failed: {result.Error}";
    }

    private static void Replace<T>(ObservableCollection<T> collection, System.Collections.Generic.IEnumerable<T> items)
    {
        collection.Clear();
        foreach (var item in items)
            collection.Add(item);
    }
}
