using System.Security.Cryptography;

namespace findamodel.Services;

public class InternalLlmModelStore(
    IConfiguration configuration,
    IHttpClientFactory httpClientFactory,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);
    private readonly SemaphoreSlim _lock = new(1, 1);
    private string? _cachedModelUrl;
    private string? _cachedPath;

    public string? ModelUrlOverride => configuration["LocalLlm:Internal:ModelUrl"];

    public string? ModelSha256 => configuration["LocalLlm:Internal:ModelSha256"];

    public async Task<string> EnsureModelAsync(string? requestedModel, CancellationToken ct)
    {
        var modelUrl = ResolveModelUrl(requestedModel);
        _logger.LogInformation(
            "Internal LLM model resolution: requestedModel={RequestedModel}, resolvedUrl={ResolvedUrl}, hasModelUrlOverride={HasModelUrlOverride}",
            string.IsNullOrWhiteSpace(requestedModel) ? "(empty)" : requestedModel,
            modelUrl,
            !string.IsNullOrWhiteSpace(ModelUrlOverride));

        if (!string.IsNullOrWhiteSpace(_cachedPath)
            && string.Equals(_cachedModelUrl, modelUrl, StringComparison.Ordinal)
            && File.Exists(_cachedPath))
        {
            _logger.LogInformation("Internal LLM cache hit: using existing model file at {Path}", _cachedPath);
            return _cachedPath;
        }

        await _lock.WaitAsync(ct);
        try
        {
            if (!string.IsNullOrWhiteSpace(_cachedPath)
                && string.Equals(_cachedModelUrl, modelUrl, StringComparison.Ordinal)
                && File.Exists(_cachedPath))
            {
                _logger.LogInformation("Internal LLM cache hit (post-lock): using existing model file at {Path}", _cachedPath);
                return _cachedPath;
            }

            var cacheDir = ResolveCacheDir();
            Directory.CreateDirectory(cacheDir);

            var targetPath = Path.Combine(cacheDir, GetModelFileName(modelUrl));
            if (File.Exists(targetPath))
            {
                if (await VerifyShaAsync(targetPath, ModelSha256, ct))
                {
                    _cachedModelUrl = modelUrl;
                    _cachedPath = targetPath;
                    _logger.LogInformation("Internal LLM cache hit (disk): verified model file at {Path}", targetPath);
                    return targetPath;
                }

                _logger.LogWarning("Existing internal LLM model cache failed checksum validation, re-downloading: {Path}", targetPath);
                File.Delete(targetPath);
            }

            _logger.LogInformation("Downloading internal LLM model from Hugging Face: {Url}", modelUrl);
            var tmpPath = targetPath + ".part";
            if (File.Exists(tmpPath))
                File.Delete(tmpPath);

            var client = httpClientFactory.CreateClient(nameof(InternalLlmModelStore));
            using var response = await client.GetAsync(modelUrl, HttpCompletionOption.ResponseHeadersRead, ct);
            response.EnsureSuccessStatusCode();

            await using (var source = await response.Content.ReadAsStreamAsync(ct))
            await using (var destination = File.Create(tmpPath))
            {
                await source.CopyToAsync(destination, ct);
            }

            if (!await VerifyShaAsync(tmpPath, ModelSha256, ct))
            {
                File.Delete(tmpPath);
                throw new InvalidOperationException("Downloaded internal LLM model failed SHA256 validation.");
            }

            File.Move(tmpPath, targetPath, overwrite: true);
            _cachedModelUrl = modelUrl;
            _cachedPath = targetPath;
            _logger.LogInformation("Internal LLM model cached at {Path}", targetPath);

            return targetPath;
        }
        finally
        {
            _lock.Release();
        }
    }

    public string ResolveModelUrl(string? requestedModel)
    {
        var configuredUrl = ModelUrlOverride?.Trim();
        if (!string.IsNullOrWhiteSpace(configuredUrl))
            return configuredUrl;

        var (modelName, aliasAppliedFrom) = NormalizeModelName(requestedModel);
        if (!string.IsNullOrWhiteSpace(aliasAppliedFrom))
        {
            _logger.LogWarning(
                "Internal LLM model alias applied: requestedModel={RequestedModel} is not directly downloadable as GGUF; using compatibleModel={CompatibleModel} instead",
                aliasAppliedFrom,
                modelName);
        }

        return $"https://huggingface.co/bartowski/{modelName}-GGUF/resolve/main/{modelName}-Q4_K_M.gguf";
    }

    private static (string ModelName, string? AliasAppliedFrom) NormalizeModelName(string? requestedModel)
    {
        if (string.IsNullOrWhiteSpace(requestedModel))
            return (AppConfigService.DefaultTagGenerationModel, null);

        var trimmed = requestedModel.Trim();
        var repoName = trimmed.Contains('/') ? trimmed[(trimmed.LastIndexOf('/') + 1)..] : trimmed;

        if (string.Equals(repoName, "qwen2.5vl:7b", StringComparison.OrdinalIgnoreCase)
            || string.Equals(repoName, "Qwen2.5-VL-7B-Instruct", StringComparison.OrdinalIgnoreCase))
            return ("Qwen2.5-7B-Instruct", repoName);

        if (string.Equals(repoName, "qwen2.5:1.5b", StringComparison.OrdinalIgnoreCase))
            return ("Qwen2.5-1.5B-Instruct", repoName);

        return (repoName, null);
    }

    private string ResolveCacheDir()
    {
        var configured = configuration["LocalLlm:Internal:CachePath"];
        if (!string.IsNullOrWhiteSpace(configured))
            return Path.GetFullPath(configured);

        var dataPath = configuration["Configuration:DataPath"] ?? "data";
        return Path.Combine(Path.GetFullPath(dataPath), "cache", "llm");
    }

    private static string GetModelFileName(string url)
    {
        var uri = new Uri(url);
        var fileName = Path.GetFileName(uri.LocalPath);
        return string.IsNullOrWhiteSpace(fileName) ? "internal-model.gguf" : fileName;
    }

    private static async Task<bool> VerifyShaAsync(string path, string? expectedSha256, CancellationToken ct)
    {
        if (string.IsNullOrWhiteSpace(expectedSha256))
            return true;

        var normalizedExpected = expectedSha256.Trim().ToLowerInvariant();
        await using var stream = File.OpenRead(path);
        var hash = await SHA256.HashDataAsync(stream, ct);
        var actual = Convert.ToHexString(hash).ToLowerInvariant();
        return string.Equals(actual, normalizedExpected, StringComparison.OrdinalIgnoreCase);
    }
}

public class InternalLlmWarmupService(
    InternalLlmModelStore modelStore,
    AppConfigService appConfigService,
    ILoggerFactory loggerFactory) : IHostedService
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        try
        {
            var config = await appConfigService.GetAsync();
            if (!string.Equals(config.TagGenerationProvider, "internal", StringComparison.OrdinalIgnoreCase))
                return;

            await modelStore.EnsureModelAsync(config.TagGenerationModel, cancellationToken);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Internal LLM warmup failed. Model will retry on first request.");
        }
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;
}
