using System.Security.Cryptography;

namespace findamodel.Services;

public class InternalLlmModelStore(
    IConfiguration configuration,
    IHttpClientFactory httpClientFactory,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);
    private readonly SemaphoreSlim _lock = new(1, 1);
    private string? _cachedPath;

    public string ModelUrl => configuration["LocalLlm:Internal:ModelUrl"]
        ?? "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf";

    public string? ModelSha256 => configuration["LocalLlm:Internal:ModelSha256"];

    public async Task<string> EnsureModelAsync(CancellationToken ct)
    {
        if (!string.IsNullOrWhiteSpace(_cachedPath) && File.Exists(_cachedPath))
            return _cachedPath;

        await _lock.WaitAsync(ct);
        try
        {
            if (!string.IsNullOrWhiteSpace(_cachedPath) && File.Exists(_cachedPath))
                return _cachedPath;

            var cacheDir = ResolveCacheDir();
            Directory.CreateDirectory(cacheDir);

            var targetPath = Path.Combine(cacheDir, GetModelFileName(ModelUrl));
            if (File.Exists(targetPath))
            {
                if (await VerifyShaAsync(targetPath, ModelSha256, ct))
                {
                    _cachedPath = targetPath;
                    return targetPath;
                }

                _logger.LogWarning("Existing internal LLM model cache failed checksum validation, re-downloading: {Path}", targetPath);
                File.Delete(targetPath);
            }

            _logger.LogInformation("Downloading internal LLM model from Hugging Face: {Url}", ModelUrl);
            var tmpPath = targetPath + ".part";
            if (File.Exists(tmpPath))
                File.Delete(tmpPath);

            var client = httpClientFactory.CreateClient(nameof(InternalLlmModelStore));
            using var response = await client.GetAsync(ModelUrl, HttpCompletionOption.ResponseHeadersRead, ct);
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
            _cachedPath = targetPath;
            _logger.LogInformation("Internal LLM model cached at {Path}", targetPath);

            return targetPath;
        }
        finally
        {
            _lock.Release();
        }
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
    ILoggerFactory loggerFactory) : IHostedService
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        try
        {
            await modelStore.EnsureModelAsync(cancellationToken);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Internal LLM warmup failed. Model will retry on first request.");
        }
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;
}
