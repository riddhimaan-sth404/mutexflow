using CommunityToolkit.Mvvm.ComponentModel;

namespace MutexFlow.Desktop.Models;

public partial class IntegrationItem : ObservableObject
{
    public string Id { get; set; } = string.Empty;
    public string Name { get; set; } = string.Empty;
    public string Category { get; set; } = string.Empty;
    public string OAuthProvider { get; set; } = string.Empty;
    public string Description { get; set; } = string.Empty;
    public string CredentialPlaceholder { get; set; } = string.Empty;
    public string TargetPlaceholder { get; set; } = string.Empty;

    [ObservableProperty]
    private bool _isSelected;

    [ObservableProperty]
    private bool _isConnected;

    [ObservableProperty]
    private bool _isConfiguring;

    [ObservableProperty]
    private string _clientId = string.Empty;

    [ObservableProperty]
    private string _apiKeyOrToken = string.Empty;

    [ObservableProperty]
    private string _targetDestination = string.Empty;

    [ObservableProperty]
    private string _statusText = "Disconnected";

    [ObservableProperty]
    private string? _lastUpdated;
}

public class GgufModelItem
{
    public string Name { get; set; } = string.Empty;
    public string Repo { get; set; } = string.Empty;
    public string FileName { get; set; } = string.Empty;
    public string SizeEstimate { get; set; } = string.Empty;

    public override string ToString() => Name;
}
