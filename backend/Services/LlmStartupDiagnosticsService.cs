namespace findamodel.Services;

public class LlmStartupDiagnosticsService(
    AppConfigService appConfigService,
    LocalLlmProviderResolver providerResolver,
    ILoggerFactory loggerFactory) : BackgroundService
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        try
        {
            var config = await appConfigService.GetAsync();
            var provider = config?.TagGenerationProvider ?? "internal";
            var llmProvider = providerResolver.Resolve(provider);

            if (llmProvider == null)
                return;

            var settings = new LocalLlmProviderSettings(
                Endpoint: config?.TagGenerationEndpoint ?? "http://localhost:11434",
                Model: config?.TagGenerationModel ?? AppConfigService.GetDefaultTagGenerationModel(),
                TimeoutMs: config?.TagGenerationTimeoutMs ?? 30000);

            var health = await llmProvider.GetHealthAsync(settings, stoppingToken);

            if (health.Reachable)
            {
                _logger.LogInformation(
                    "LLM startup check PASSED: provider={Provider}, model={Model}, backend={Backend}",
                    health.Provider,
                    health.Model,
                    health.Metadata?.GetValueOrDefault("backend") ?? "unknown");
            }
            else
            {
                _logger.LogWarning(
                    "LLM startup check FAILED: provider={Provider}, error={Error}. Tag generation may be unavailable.",
                    health.Provider,
                    health.Error ?? "unknown error");
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "LLM startup health check encountered an error. Ensure LLamaSharp native backends are installed.");
        }
    }
}
