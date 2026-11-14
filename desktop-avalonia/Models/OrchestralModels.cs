using System;
using System.Text.Json.Serialization;
using System.Windows.Input;

namespace MutexFlow.Desktop.Models;

public sealed class OrchestralApproval
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;
    [JsonPropertyName("agent_id")] public string AgentId { get; set; } = string.Empty;
    [JsonPropertyName("tool_name")] public string ToolName { get; set; } = string.Empty;
    [JsonPropertyName("target")] public string Target { get; set; } = string.Empty;
    [JsonPropertyName("command_preview")] public string CommandPreview { get; set; } = string.Empty;
    [JsonPropertyName("risk_tier")] public string RiskTier { get; set; } = string.Empty;
    [JsonPropertyName("status")] public string Status { get; set; } = string.Empty;
    [JsonIgnore] public ICommand? ApproveCommand { get; set; }
    [JsonIgnore] public ICommand? DenyCommand { get; set; }
}

public sealed class OrchestralAgent
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;
    [JsonPropertyName("name")] public string Name { get; set; } = string.Empty;
    [JsonPropertyName("role")] public string Role { get; set; } = string.Empty;
    [JsonPropertyName("status")] public string Status { get; set; } = string.Empty;
    [JsonPropertyName("current_task")] public string? CurrentTask { get; set; }
}

public sealed class OrchestralFinding
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;
    [JsonPropertyName("title")] public string Title { get; set; } = string.Empty;
    [JsonPropertyName("target")] public string Target { get; set; } = string.Empty;
    [JsonPropertyName("severity")] public string Severity { get; set; } = string.Empty;
    [JsonPropertyName("description")] public string Description { get; set; } = string.Empty;
    [JsonPropertyName("evidence")] public string? Evidence { get; set; }
}

public sealed class OrchestralAuditEntry
{
    [JsonPropertyName("actor")] public string Actor { get; set; } = string.Empty;
    [JsonPropertyName("action")] public string Action { get; set; } = string.Empty;
    [JsonPropertyName("details")] public string Details { get; set; } = string.Empty;
    [JsonPropertyName("timestamp")] public DateTimeOffset Timestamp { get; set; }
    [JsonPropertyName("entry_hash")] public string? EntryHash { get; set; }
}
