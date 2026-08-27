using System;
using System.Collections.Generic;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using MutexFlow.Desktop.Models;

namespace MutexFlow.Desktop.Services;

public interface IAgentClientService
{
    string BackendUrl { get; set; }
    Task<(bool Ok, string? Error)> VerifyLicenseAsync(string licenseKey, string machineId, CancellationToken ct = default);
    Task<(bool Ok, string? Result, string? Error)> RunLocalAsync(string prompt, WorkflowType workflow, TaskComplexity complexity, CancellationToken ct = default);
    Task<(bool Ok, string? Result, string? Error)> RunStandardAsync(string prompt, string licenseKey, string machineId, WorkflowType workflow, TaskComplexity complexity, List<string>? targetIntegrations = null, CancellationToken ct = default);
    Task<(bool Ok, string? Result, string? Error)> RunByokAsync(string prompt, string byokKey, WorkflowType workflow, TaskComplexity complexity, CancellationToken ct = default);
    Task<(bool Ok, string? Error)> DispatchDirectIntegrationAsync(IntegrationItem integration, string result, string prompt, WorkflowType workflow, TaskComplexity complexity, CancellationToken ct = default);
    Task<bool> DispatchIntegrationsAsync(string result, string? prompt, string? licenseKey, string? machineId, WorkflowType workflow, TaskComplexity complexity, List<string> targetIntegrations, CancellationToken ct = default);
    Task<List<IntegrationItem>> GetIntegrationStatusesAsync(string machineId, CancellationToken ct = default);
}

public class AgentClientService : IAgentClientService
{
    private readonly HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(90) };
    private readonly ISecureStorageService _storage;

    public string BackendUrl { get; set; } = "http://localhost:3001";

    public AgentClientService(ISecureStorageService storage)
    {
        _storage = storage;
        var savedUrl = _storage.GetSecret("backend_url");
        if (!string.IsNullOrWhiteSpace(savedUrl))
            BackendUrl = savedUrl;
    }

    public async Task<(bool Ok, string? Error)> VerifyLicenseAsync(string licenseKey, string machineId, CancellationToken ct = default)
    {
        try
        {
            var payload = new { licenseKey, machineId };
            var resp = await _http.PostAsJsonAsync($"{BackendUrl}/api/v1/auth/verify-license", payload, ct);
            var json = await resp.Content.ReadFromJsonAsync<JsonObject>(cancellationToken: ct);

            if (json != null && json["ok"]?.GetValue<bool>() == true)
                return (true, null);

            var err = json?["error"]?.GetValue<string>() ?? $"HTTP {resp.StatusCode}";
            return (false, err);
        }
        catch
        {
            // In local-first offline mode, permit license verification if server is unreachable
            return (true, null);
        }
    }

    public async Task<(bool Ok, string? Result, string? Error)> RunLocalAsync(
        string prompt,
        WorkflowType workflow,
        TaskComplexity complexity,
        CancellationToken ct = default)
    {
        try
        {
            var systemPrompt = workflow switch
            {
                WorkflowType.DataExtraction => "You are a data extraction assistant. Extract key entities, dates, and amounts into structured JSON.",
                WorkflowType.ProfessionalDrafting => "You are a professional writing assistant. Draft clear, natural, and thoughtful messages.",
                _ => "You are MutexFlow, a helpful and precise local AI assistant. Think step by step and provide actionable answers."
            };

            var messages = new[]
            {
                new { role = "system", content = systemPrompt },
                new { role = "user", content = prompt }
            };

            var payload = new { model = "local-model", messages, temperature = 0.7 };
            var resp = await _http.PostAsJsonAsync("http://localhost:8080/v1/chat/completions", payload, ct);

            if (!resp.IsSuccessStatusCode)
                return (false, null, $"Local engine returned HTTP {resp.StatusCode}");

            var json = await resp.Content.ReadFromJsonAsync<JsonObject>(cancellationToken: ct);
            var content = json?["choices"]?[0]?["message"]?["content"]?.GetValue<string>();

            if (string.IsNullOrWhiteSpace(content))
                return (false, null, "Local AI returned an empty response.");

            return (true, content, null);
        }
        catch (Exception ex)
        {
            return (false, null, $"Cannot connect to local AI on http://localhost:8080: {ex.Message}");
        }
    }

    public async Task<(bool Ok, string? Result, string? Error)> RunStandardAsync(
        string prompt,
        string licenseKey,
        string machineId,
        WorkflowType workflow,
        TaskComplexity complexity,
        List<string>? targetIntegrations = null,
        CancellationToken ct = default)
    {
        try
        {
            var payload = new
            {
                prompt,
                licenseKey,
                machineId,
                workflowType = workflow.ToString().ToLowerInvariant(),
                taskComplexity = complexity.ToString().ToLowerInvariant(),
                targetIntegrations = targetIntegrations ?? new List<string>()
            };

            var resp = await _http.PostAsJsonAsync($"{BackendUrl}/api/v1/agent/process-standard", payload, ct);
            var json = await resp.Content.ReadFromJsonAsync<JsonObject>(cancellationToken: ct);

            if (json != null && json["ok"]?.GetValue<bool>() == true)
            {
                var result = json["result"]?.GetValue<string>() ?? string.Empty;
                return (true, result, null);
            }

            var err = json?["error"]?.GetValue<string>() ?? $"HTTP {resp.StatusCode}";
            return (false, null, err);
        }
        catch (Exception ex)
        {
            return (false, null, $"Cloud agent error: {ex.Message}");
        }
    }

    public async Task<(bool Ok, string? Result, string? Error)> RunByokAsync(
        string prompt,
        string byokKey,
        WorkflowType workflow,
        TaskComplexity complexity,
        CancellationToken ct = default)
    {
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Post, "https://openrouter.ai/api/v1/chat/completions");
            req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", byokKey);

            var messages = new[]
            {
                new { role = "system", content = $"You are MutexFlow, an enterprise assistant. Workflow: {workflow}." },
                new { role = "user", content = prompt }
            };

            req.Content = JsonContent.Create(new { model = "openai/gpt-4o-mini", messages });

            var resp = await _http.SendAsync(req, ct);
            if (!resp.IsSuccessStatusCode)
            {
                var errBody = await resp.Content.ReadAsStringAsync(ct);
                return (false, null, $"OpenRouter HTTP {resp.StatusCode}: {errBody}");
            }

            var json = await resp.Content.ReadFromJsonAsync<JsonObject>(cancellationToken: ct);
            var content = json?["choices"]?[0]?["message"]?.GetValue<string>();

            if (string.IsNullOrWhiteSpace(content))
            {
                content = json?["choices"]?[0]?["message"]?["content"]?.GetValue<string>();
            }

            if (string.IsNullOrWhiteSpace(content))
                return (false, null, "OpenRouter returned empty response.");

            return (true, content, null);
        }
        catch (Exception ex)
        {
            return (false, null, $"OpenRouter error: {ex.Message}");
        }
    }

    public async Task<(bool Ok, string? Error)> DispatchDirectIntegrationAsync(
        IntegrationItem integration,
        string result,
        string prompt,
        WorkflowType workflow,
        TaskComplexity complexity,
        CancellationToken ct = default)
    {
        var token = integration.ApiKeyOrToken.Trim();
        var destination = integration.TargetDestination.Trim();

        try
        {
            if (integration.Id == "slack")
            {
                if (token.StartsWith("https://hooks.slack.com"))
                {
                    var payload = new { text = $"*MutexFlow Result*\n```{result}```" };
                    var resp = await _http.PostAsJsonAsync(token, payload, ct);
                    return resp.IsSuccessStatusCode ? (true, null) : (false, $"Slack webhook HTTP {resp.StatusCode}");
                }
                else if (token.StartsWith("xoxb-"))
                {
                    using var req = new HttpRequestMessage(HttpMethod.Post, "https://slack.com/api/chat.postMessage");
                    req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
                    req.Content = JsonContent.Create(new
                    {
                        channel = string.IsNullOrEmpty(destination) ? "#general" : destination,
                        text = $"*MutexFlow Result*\n```{result}```"
                    });
                    var resp = await _http.SendAsync(req, ct);
                    return resp.IsSuccessStatusCode ? (true, null) : (false, $"Slack API HTTP {resp.StatusCode}");
                }
            }
            else if (integration.Id == "github")
            {
                var repo = string.IsNullOrEmpty(destination) ? "owner/repo" : destination;
                using var req = new HttpRequestMessage(HttpMethod.Post, $"https://api.github.com/repos/{repo}/issues");
                req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
                req.Headers.UserAgent.Add(new ProductInfoHeaderValue("MutexFlow-Desktop", "1.0"));
                req.Content = JsonContent.Create(new
                {
                    title = $"MutexFlow Result: {workflow} - {DateTime.UtcNow:yyyy-MM-dd HH:mm}",
                    body = $"## Agent Output\n\n**Prompt:** {prompt}\n\n### Result\n```\n{result}\n```"
                });
                var resp = await _http.SendAsync(req, ct);
                return resp.IsSuccessStatusCode ? (true, null) : (false, $"GitHub API HTTP {resp.StatusCode}");
            }

            return (true, null);
        }
        catch (Exception ex)
        {
            return (false, ex.Message);
        }
    }

    public async Task<bool> DispatchIntegrationsAsync(
        string result,
        string? prompt,
        string? licenseKey,
        string? machineId,
        WorkflowType workflow,
        TaskComplexity complexity,
        List<string> targetIntegrations,
        CancellationToken ct = default)
    {
        try
        {
            var payload = new
            {
                result,
                prompt,
                licenseKey,
                machineId,
                workflowType = workflow.ToString().ToLowerInvariant(),
                taskComplexity = complexity.ToString().ToLowerInvariant(),
                targetIntegrations
            };

            var resp = await _http.PostAsJsonAsync($"{BackendUrl}/api/v1/integrations/dispatch", payload, ct);
            return resp.IsSuccessStatusCode;
        }
        catch
        {
            return false;
        }
    }

    public Task<List<IntegrationItem>> GetIntegrationStatusesAsync(string machineId, CancellationToken ct = default)
    {
        return Task.FromResult(new List<IntegrationItem>());
    }
}
