using System;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading.Tasks;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using MutexFlow.Desktop.Models;
using MutexFlow.Desktop.Services;

namespace MutexFlow.Desktop.ViewModels;

public partial class IntegrationsViewModel : ViewModelBase
{
    private readonly IAgentClientService _agentClient;
    private readonly IOAuthService _oauthService;
    private readonly IHardwareFingerprintService _fingerprintService;
    private readonly ISecureStorageService _storage;

    [ObservableProperty]
    private string _searchQuery = string.Empty;

    public ObservableCollection<IntegrationItem> Integrations { get; } = new();
    public ObservableCollection<IntegrationItem> FilteredIntegrations { get; } = new();

    public IntegrationsViewModel(
        IAgentClientService agentClient,
        IOAuthService oauthService,
        IHardwareFingerprintService fingerprintService,
        ISecureStorageService storage)
    {
        _agentClient = agentClient;
        _oauthService = oauthService;
        _fingerprintService = fingerprintService;
        _storage = storage;

        InitializeIntegrations();
        ApplyFilter();
    }

    private void InitializeIntegrations()
    {
        var defaultItems = new[]
        {
            new IntegrationItem
            {
                Id = "slack",
                Name = "Slack",
                Category = "Communications",
                OAuthProvider = "slack",
                Description = "Post agent results to your Slack channel or webhook.",
                CredentialPlaceholder = "Bot Token (xoxb-...) or Webhook URL",
                TargetPlaceholder = "#channel-name (e.g. #general)"
            },
            new IntegrationItem
            {
                Id = "github",
                Name = "GitHub",
                Category = "Engineering & Product",
                OAuthProvider = "github",
                Description = "Create issues and automated summaries in your repository.",
                CredentialPlaceholder = "Personal Access Token (ghp_...)",
                TargetPlaceholder = "owner/repository (e.g. org/repo)"
            },
            new IntegrationItem
            {
                Id = "google_sheets",
                Name = "Google Sheets",
                Category = "Knowledge & Data",
                OAuthProvider = "google",
                Description = "Append prompt & response logs to your spreadsheet.",
                CredentialPlaceholder = "Google OAuth Client ID or API Key",
                TargetPlaceholder = "Spreadsheet ID"
            },
            new IntegrationItem
            {
                Id = "linear",
                Name = "Linear",
                Category = "Engineering & Product",
                OAuthProvider = "linear",
                Description = "Create issues and project tasks from AI outputs.",
                CredentialPlaceholder = "Linear API Key (lin_api_...)",
                TargetPlaceholder = "Team Key (e.g. ENG)"
            },
            new IntegrationItem
            {
                Id = "jira",
                Name = "Jira",
                Category = "Engineering & Product",
                OAuthProvider = "jira",
                Description = "Log work and create Jira tickets from agent work.",
                CredentialPlaceholder = "Atlassian API Token",
                TargetPlaceholder = "Project Key (e.g. PROJ)"
            },
            new IntegrationItem
            {
                Id = "notion",
                Name = "Notion",
                Category = "Knowledge & Data",
                OAuthProvider = "notion",
                Description = "Sync agent outputs directly to your Notion database.",
                CredentialPlaceholder = "Internal Integration Secret (secret_...)",
                TargetPlaceholder = "Database ID"
            },
            new IntegrationItem
            {
                Id = "confluence",
                Name = "Confluence",
                Category = "Knowledge & Data",
                OAuthProvider = "confluence",
                Description = "Publish generated documents and notes to Confluence spaces.",
                CredentialPlaceholder = "Atlassian API Token",
                TargetPlaceholder = "Space Key (e.g. DOCS)"
            },
            new IntegrationItem
            {
                Id = "hubspot",
                Name = "HubSpot",
                Category = "CRM & Sales",
                OAuthProvider = "hubspot",
                Description = "Sync drafts and extracted contact records into HubSpot.",
                CredentialPlaceholder = "Private App Access Token (pat-na1-...)",
                TargetPlaceholder = "Default Pipeline / Owner ID"
            },
            new IntegrationItem
            {
                Id = "salesforce",
                Name = "Salesforce",
                Category = "CRM & Sales",
                OAuthProvider = "salesforce",
                Description = "Create leads and log AI interactions directly in Salesforce.",
                CredentialPlaceholder = "Connected App Client ID",
                TargetPlaceholder = "Instance URL"
            }
        };

        foreach (var item in defaultItems)
        {
            // Restore saved credentials from DPAPI storage
            var savedToken = _storage.GetSecret($"integration_{item.Id}_token");
            var savedTarget = _storage.GetSecret($"integration_{item.Id}_target");
            var savedClientId = _storage.GetSecret($"integration_{item.Id}_clientid");

            if (!string.IsNullOrEmpty(savedToken))
            {
                item.ApiKeyOrToken = savedToken;
                item.IsConnected = true;
                item.StatusText = "Connected";
            }

            if (!string.IsNullOrEmpty(savedTarget))
                item.TargetDestination = savedTarget;

            if (!string.IsNullOrEmpty(savedClientId))
                item.ClientId = savedClientId;

            Integrations.Add(item);
        }
    }

    partial void OnSearchQueryChanged(string value)
    {
        ApplyFilter();
    }

    private void ApplyFilter()
    {
        FilteredIntegrations.Clear();
        var query = SearchQuery.Trim().ToLowerInvariant();

        foreach (var item in Integrations)
        {
            if (string.IsNullOrEmpty(query) ||
                item.Name.ToLowerInvariant().Contains(query) ||
                item.Category.ToLowerInvariant().Contains(query))
            {
                FilteredIntegrations.Add(item);
            }
        }
    }

    [RelayCommand]
    public void ToggleConfigure(IntegrationItem item)
    {
        item.IsConfiguring = !item.IsConfiguring;
    }

    [RelayCommand]
    public void SaveIntegrationCredentials(IntegrationItem item)
    {
        if (!string.IsNullOrWhiteSpace(item.ApiKeyOrToken))
        {
            _storage.SaveSecret($"integration_{item.Id}_token", item.ApiKeyOrToken.Trim());
            item.IsConnected = true;
            item.StatusText = "Connected";
        }
        else
        {
            _storage.RemoveSecret($"integration_{item.Id}_token");
            item.IsConnected = false;
            item.StatusText = "Disconnected";
        }

        if (!string.IsNullOrWhiteSpace(item.TargetDestination))
            _storage.SaveSecret($"integration_{item.Id}_target", item.TargetDestination.Trim());

        if (!string.IsNullOrWhiteSpace(item.ClientId))
            _storage.SaveSecret($"integration_{item.Id}_clientid", item.ClientId.Trim());

        item.IsConfiguring = false;
    }

    [RelayCommand]
    public async Task ConnectOAuthAsync(IntegrationItem item)
    {
        item.StatusText = "Connecting...";
        var machineId = _fingerprintService.GetMachineId();
        var success = await _oauthService.StartOAuthAsync(item.OAuthProvider, machineId, _agentClient.BackendUrl);

        if (success)
        {
            item.IsConnected = true;
            item.StatusText = "Connected";
        }
        else
        {
            item.StatusText = "Disconnected";
        }
    }
}
