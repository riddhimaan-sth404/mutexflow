using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using MutexFlow.Desktop.Services;
using MutexFlow.Desktop.ViewModels;
using MutexFlow.Desktop.Views;

namespace MutexFlow.Desktop;

public partial class App : Application
{
    private ILlamaProcessService? _llamaService;

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var storage = new SecureStorageService();
            var fingerprint = new HardwareFingerprintService();
            _llamaService = new LlamaProcessService();
            var oauth = new OAuthService();
            var agentClient = new AgentClientService(storage);

            var integrationsVm = new IntegrationsViewModel(agentClient, oauth, fingerprint, storage);
            var localAiVm = new LocalAiViewModel(_llamaService);
            var agentVm = new AgentViewModel(agentClient, fingerprint, storage, _llamaService, integrationsVm, localAiVm);
            var settingsVm = new SettingsViewModel(agentClient, storage, fingerprint);

            var mainVm = new MainWindowViewModel(agentVm, integrationsVm, localAiVm, settingsVm);

            desktop.MainWindow = new MainWindow
            {
                DataContext = mainVm
            };

            desktop.Exit += (s, e) =>
            {
                _llamaService?.StopLlama();
            };
        }

        base.OnFrameworkInitializationCompleted();
    }
}
