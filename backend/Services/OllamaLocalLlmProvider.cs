using System.Diagnostics;
using System.Net.Http.Json;
using System.Text.Json;

namespace findamodel.Services;

public class OllamaLocalLlmProvider(
    IHttpClientFactory httpClientFactory,
    ILoggerFactory loggerFactory) : ILocalLlmProvider
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);

    public string Name => "ollama";

    public async Task<LocalLlmHealth> GetHealthAsync(LocalLlmProviderSettings settings, CancellationToken ct)
    {
        try
        {
            using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
            linkedCts.CancelAfter(TimeSpan.FromMilliseconds(Math.Max(1_000, settings.TimeoutMs)));

            var client = httpClientFactory.CreateClient(nameof(OllamaLocalLlmProvider));
            var baseUrl = NormalizeEndpoint(settings.Endpoint);
            using var response = await client.GetAsync($"{baseUrl}/api/tags", linkedCts.Token);
            if (!response.IsSuccessStatusCode)
            {
                return new LocalLlmHealth(false, false, Name, settings.Model, $"HTTP {(int)response.StatusCode}");
            }

            var content = await response.Content.ReadAsStringAsync(linkedCts.Token);
            var modelReady = content.Contains($"\"name\":\"{settings.Model}\"", StringComparison.OrdinalIgnoreCase);

            return new LocalLlmHealth(
                Reachable: true,
                ModelReady: modelReady,
                Provider: Name,
                Model: settings.Model,
                Metadata: new Dictionary<string, string> { ["endpoint"] = baseUrl });
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Ollama health check failed for endpoint {Endpoint}", settings.Endpoint);
            return new LocalLlmHealth(false, false, Name, settings.Model, ex.Message);
        }
    }

    public async Task<LocalLlmResponse> GenerateAsync(LocalLlmProviderSettings settings, LocalLlmRequest request, CancellationToken ct)
    {
        var sw = Stopwatch.StartNew();
        using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        linkedCts.CancelAfter(TimeSpan.FromMilliseconds(Math.Max(1_000, settings.TimeoutMs)));

        var client = httpClientFactory.CreateClient(nameof(OllamaLocalLlmProvider));
        var baseUrl = NormalizeEndpoint(settings.Endpoint);

        var payload = new Dictionary<string, object?>
        {
            ["model"] = settings.Model,
            ["system"] = request.SystemPrompt,
            ["prompt"] = request.UserPrompt,
            ["stream"] = false,
            ["format"] = "json",
            ["options"] = new Dictionary<string, object?>
            {
                ["temperature"] = 0.1,
                ["num_predict"] = Math.Max(64, request.MaxOutputTokens),
            },
        };

        var images = await BuildImagesAsync(request.ImagePath, linkedCts.Token);
        if (images.Count > 0)
            payload["images"] = images;

        using var response = await client.PostAsJsonAsync($"{baseUrl}/api/generate", payload, linkedCts.Token);
        var rawPayload = await response.Content.ReadAsStringAsync(linkedCts.Token);
        response.EnsureSuccessStatusCode();

        sw.Stop();
        var modelText = ExtractModelText(rawPayload);
        if (string.IsNullOrWhiteSpace(modelText))
        {
            return LocalLlmResponse.Empty(Name, settings.Model, sw.Elapsed) with
            {
                RawResponse = rawPayload,
            };
        }

        return request.TaskKind switch
        {
            LocalLlmTaskKind.Tags => ParseTagResponse(settings.Model, modelText, rawPayload, sw.Elapsed),
            LocalLlmTaskKind.Description => ParseDescriptionResponse(settings.Model, modelText, rawPayload, sw.Elapsed),
            _ => LocalLlmResponse.Empty(Name, settings.Model, sw.Elapsed) with { RawResponse = rawPayload },
        };
    }

    private static LocalLlmResponse ParseTagResponse(string model, string modelText, string rawPayload, TimeSpan latency)
    {
        var tags = new List<string>();
        var confidence = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        string? notes = null;

        if (TryParseJsonObject(modelText, out var doc))
        {
            using (doc)
            {
                if (doc.RootElement.TryGetProperty("tags", out var tagsEl) && tagsEl.ValueKind == JsonValueKind.Array)
                {
                    foreach (var item in tagsEl.EnumerateArray())
                    {
                        if (item.ValueKind != JsonValueKind.String)
                            continue;

                        var tag = item.GetString();
                        if (!string.IsNullOrWhiteSpace(tag))
                            tags.Add(tag.Trim());
                    }
                }

                if (doc.RootElement.TryGetProperty("confidence", out var confEl) && confEl.ValueKind == JsonValueKind.Object)
                {
                    foreach (var prop in confEl.EnumerateObject())
                    {
                        if (prop.Value.ValueKind == JsonValueKind.Number && prop.Value.TryGetSingle(out var score))
                            confidence[prop.Name] = score;
                    }
                }

                if (doc.RootElement.TryGetProperty("notes", out var notesEl) && notesEl.ValueKind == JsonValueKind.String)
                    notes = notesEl.GetString();
            }
        }

        return new LocalLlmResponse
        {
            Provider = "ollama",
            Model = model,
            Tags = tags,
            Confidence = confidence,
            Notes = notes,
            RawResponse = rawPayload,
            Latency = latency,
        };
    }

    private static LocalLlmResponse ParseDescriptionResponse(string model, string modelText, string rawPayload, TimeSpan latency)
    {
        string? description = null;
        var confidence = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);

        if (TryParseJsonObject(modelText, out var doc))
        {
            using (doc)
            {
                if (doc.RootElement.TryGetProperty("description", out var descriptionEl) && descriptionEl.ValueKind == JsonValueKind.String)
                    description = descriptionEl.GetString()?.Trim();

                if (doc.RootElement.TryGetProperty("confidence", out var confEl) && confEl.ValueKind == JsonValueKind.Number && confEl.TryGetSingle(out var score))
                    confidence["description"] = score;
            }
        }

        description ??= modelText.Trim();
        return new LocalLlmResponse
        {
            Provider = "ollama",
            Model = model,
            Description = description,
            Confidence = confidence,
            RawResponse = rawPayload,
            Latency = latency,
        };
    }

    private static string NormalizeEndpoint(string endpoint)
    {
        var value = string.IsNullOrWhiteSpace(endpoint)
            ? "http://localhost:11434"
            : endpoint.Trim();

        return value.TrimEnd('/');
    }

    private static string ExtractModelText(string rawPayload)
    {
        if (!TryParseJsonObject(rawPayload, out var doc))
            return string.Empty;

        using (doc)
        {
            if (doc.RootElement.TryGetProperty("response", out var responseEl) && responseEl.ValueKind == JsonValueKind.String)
                return responseEl.GetString() ?? string.Empty;
        }

        return string.Empty;
    }

    private static bool TryParseJsonObject(string raw, out JsonDocument doc)
    {
        try
        {
            var json = ExtractJsonObject(raw);
            doc = JsonDocument.Parse(json);
            return true;
        }
        catch
        {
            doc = null!;
            return false;
        }
    }

    private static string ExtractJsonObject(string raw)
    {
        var start = raw.IndexOf('{');
        var end = raw.LastIndexOf('}');
        if (start >= 0 && end > start)
            return raw[start..(end + 1)];

        return raw;
    }

    private static async Task<List<string>> BuildImagesAsync(string? imagePath, CancellationToken ct)
    {
        if (string.IsNullOrWhiteSpace(imagePath) || !File.Exists(imagePath))
            return [];

        var bytes = await File.ReadAllBytesAsync(imagePath, ct);
        return [Convert.ToBase64String(bytes)];
    }
}
