using System.Text.Json;

namespace findamodel.Services;

public enum LocalLlmTaskKind
{
    Tags,
    Description,
}

public sealed record LocalLlmRequest
{
    public required LocalLlmTaskKind TaskKind { get; init; }
    public required string SystemPrompt { get; init; }
    public required string UserPrompt { get; init; }
    public IReadOnlyList<string>? AllowedTags { get; init; }
    public IReadOnlyDictionary<string, string?>? Context { get; init; }
    public string? ImagePath { get; init; }
    public int MaxTags { get; init; } = 12;
    public int MaxOutputTokens { get; init; } = 512;
}

public sealed record LocalLlmProviderSettings(
    string Endpoint,
    string Model,
    int TimeoutMs);

public sealed record LocalLlmResponse
{
    public string Provider { get; init; } = "";
    public string Model { get; init; } = "";
    public IReadOnlyList<string> Tags { get; init; } = [];
    public IReadOnlyDictionary<string, float> Confidence { get; init; } = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
    public string? Description { get; init; }
    public string? Notes { get; init; }
    public string RawResponse { get; init; } = "";
    public TimeSpan Latency { get; init; }

    public static LocalLlmResponse Empty(string provider, string model, TimeSpan latency) =>
        new()
        {
            Provider = provider,
            Model = model,
            RawResponse = JsonSerializer.Serialize(new { tags = Array.Empty<string>() }),
            Latency = latency,
        };
}

public sealed record LocalLlmHealth(
    bool Reachable,
    bool ModelReady,
    string Provider,
    string Model,
    string? Error = null,
    IReadOnlyDictionary<string, string>? Metadata = null);

public interface ILocalLlmProvider
{
    string Name { get; }
    Task<LocalLlmHealth> GetHealthAsync(LocalLlmProviderSettings settings, CancellationToken ct);
    Task<LocalLlmResponse> GenerateAsync(LocalLlmProviderSettings settings, LocalLlmRequest request, CancellationToken ct);
}
