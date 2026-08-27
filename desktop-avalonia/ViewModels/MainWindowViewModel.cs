using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace MutexFlow.Desktop.ViewModels;

public partial class MainWindowViewModel : ViewModelBase
{
    public AgentViewModel AgentVm { get; }
    public IntegrationsViewModel IntegrationsVm { get; }
    public LocalAiViewModel LocalAiVm { get; }
    public SettingsViewModel SettingsVm { get; }

    [ObservableProperty]
    private ViewModelBase _currentView;

    [ObservableProperty]
    private string _activeSection = "agent";

    public MainWindowViewModel(
        AgentViewModel agentVm,
        IntegrationsViewModel integrationsVm,
        LocalAiViewModel localAiVm,
        SettingsViewModel settingsVm)
    {
        AgentVm = agentVm;
        IntegrationsVm = integrationsVm;
        LocalAiVm = localAiVm;
        SettingsVm = settingsVm;

        _currentView = AgentVm;
    }

    [RelayCommand]
    public void Navigate(string section)
    {
        ActiveSection = section;
        CurrentView = section switch
        {
            "integrations" => IntegrationsVm,
            "local-ai" => LocalAiVm,
            "settings" => SettingsVm,
            _ => AgentVm
        };
    }
}
