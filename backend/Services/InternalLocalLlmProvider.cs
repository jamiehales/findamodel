using System.Text;
using System.Text.Json;
using LLama;
using LLama.Common;
using LLama.Sampling;

namespace findamodel.Services;

public class InternalLocalLlmProvider(
    InternalLlmModelStore modelStore,
    IConfiguration configuration,
    ILoggerFactory loggerFactory) : ILocalLlmProvider
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);
    private readonly SemaphoreSlim _modelLock = new(1, 1);
    private string? _loadedModelPath;
    private LLamaWeights? _weights;
    private ModelParams? _modelParams;

    public string Name => "internal";

    public async Task<LocalLlmHealth> GetHealthAsync(LocalLlmProviderSettings settings, CancellationToken ct)
    {
        try
        {
            var modelPath = await modelStore.EnsureModelAsync(settings.Model, ct);
            await EnsureModelLoadedAsync(modelPath, ct);

            return new LocalLlmHealth(
                Reachable: true,
                ModelReady: true,
                Provider: Name,
                Model: Path.GetFileName(modelPath),
                Metadata: new Dictionary<string, string>
                {
                    ["backend"] = (_modelParams?.GpuLayerCount ?? 0) > 0 ? "llamasharp-gpu" : "llamasharp-cpu",
                    ["modelPath"] = modelPath,
                    ["supportsVision"] = "false",
                    ["supportsDescriptions"] = "true",
                });
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Internal local LLM health check failed");
            return new LocalLlmHealth(false, false, Name, "internal", ex.Message);
        }
    }

    public async Task<LocalLlmResponse> GenerateAsync(LocalLlmProviderSettings settings, LocalLlmRequest request, CancellationToken ct)
    {
        var modelPath = await modelStore.EnsureModelAsync(settings.Model, ct);
        await EnsureModelLoadedAsync(modelPath, ct);

        _logger.LogDebug(
            "Internal LLM GenerateAsync started: task={TaskKind}, model={Model}, hasImage={HasImage}, allowedTags={AllowedTagCount}",
            request.TaskKind,
            Path.GetFileName(modelPath),
            !string.IsNullOrWhiteSpace(request.ImagePath),
            request.AllowedTags?.Count ?? 0);

        var combinedPrompt = BuildPrompt(request);
        _logger.LogDebug(
            "Internal LLM BuildPrompt complete: task={TaskKind}, promptLength={PromptLength}",
            request.TaskKind,
            combinedPrompt.Length);

        var modelOutput = await RunCompletionAsync(combinedPrompt, request.MaxOutputTokens, ct);

        return request.TaskKind switch
        {
            LocalLlmTaskKind.Tags => ParseTagResponse(Path.GetFileName(modelPath), modelOutput),
            LocalLlmTaskKind.Description => ParseDescriptionResponse(Path.GetFileName(modelPath), modelOutput),
            _ => LocalLlmResponse.Empty(Name, Path.GetFileName(modelPath), TimeSpan.Zero),
        };
    }

    private async Task EnsureModelLoadedAsync(string modelPath, CancellationToken ct)
    {
        if (_weights != null && string.Equals(_loadedModelPath, modelPath, StringComparison.OrdinalIgnoreCase))
            return;

        await _modelLock.WaitAsync(ct);
        try
        {
            if (_weights != null && string.Equals(_loadedModelPath, modelPath, StringComparison.OrdinalIgnoreCase))
                return;

            if (_weights != null && !string.Equals(_loadedModelPath, modelPath, StringComparison.OrdinalIgnoreCase))
            {
                _logger.LogInformation(
                    "Internal LLM model change detected - reinitializing runtime from {OldModelPath} to {NewModelPath}",
                    _loadedModelPath,
                    modelPath);
                (_weights as IDisposable)?.Dispose();
                _weights = null;
                _modelParams = null;
            }

            var useGpu = configuration.GetValue("LocalLlm:Internal:UseGpu", true);
            var gpuLayers = Math.Max(0, configuration.GetValue("LocalLlm:Internal:GpuLayerCount", 35));

            try
            {
                _modelParams = CreateModelParams(modelPath, useGpu ? gpuLayers : 0);
                _weights = LLamaWeights.LoadFromFile(_modelParams);
            }
            catch (Exception ex) when (useGpu && gpuLayers > 0)
            {
                _logger.LogWarning(ex, "Internal LLM GPU initialization failed; falling back to CPU mode.");
                _modelParams = CreateModelParams(modelPath, 0);
                _weights = LLamaWeights.LoadFromFile(_modelParams);
            }

            _logger.LogInformation(
                "Internal LLamaSharp model loaded from {ModelPath} (GpuLayerCount={GpuLayerCount})",
                modelPath,
                _modelParams.GpuLayerCount);
            _loadedModelPath = modelPath;
        }
        finally
        {
            _modelLock.Release();
        }
    }

    private async Task<string> RunCompletionAsync(string prompt, int requestedMaxOutputTokens, CancellationToken ct)
    {
        if (_weights == null || _modelParams == null)
            throw new InvalidOperationException("Internal model is not loaded.");

        using var context = _weights.CreateContext(_modelParams);
        var executor = new InteractiveExecutor(context);

        var history = new ChatHistory();
        history.AddMessage(
            AuthorRole.System,
            "You are a precise assistant. Return exactly one valid JSON object and nothing else. " +
            "Do not include markdown fences, the word json, or commentary. " +
            "Your response must start with '{' and end with '}'.");

        var session = new ChatSession(executor, history);
        var sb = new StringBuilder();
        var inferenceParams = CreateInferenceParams(requestedMaxOutputTokens);

        await foreach (var chunk in session.ChatAsync(new ChatHistory.Message(AuthorRole.User, prompt), inferenceParams).WithCancellation(ct))
        {
            sb.Append(chunk);
        }

        return sb.ToString().Trim();
    }

    internal static string BuildPrompt(LocalLlmRequest request)
    {
        var sb = new StringBuilder();
        sb.AppendLine(request.SystemPrompt);
        sb.AppendLine();

        if (request.Context != null && request.Context.Count > 0)
        {
            sb.AppendLine("Context:");
            foreach (var (key, value) in request.Context)
            {
                if (!string.IsNullOrWhiteSpace(value))
                    sb.AppendLine($"- {key}: {value}");
            }
            sb.AppendLine();
        }

        if (!string.IsNullOrWhiteSpace(request.ImagePath))
        {
            sb.AppendLine("Image context note:");
            sb.AppendLine("- A preview image exists but the internal text-only model cannot directly inspect pixels.");
            sb.AppendLine();
        }

        if (request.TaskKind == LocalLlmTaskKind.Tags)
        {
            sb.AppendLine("Task: choose tags from allowed list only.");
            if (request.AllowedTags != null)
                sb.AppendLine($"Allowed tags: {string.Join(", ", request.AllowedTags)}");
            sb.AppendLine("Respond with JSON exactly: {\"tags\":[\"...\"],\"confidence\":{\"tag\":0.0},\"notes\":\"optional\"}");
            sb.AppendLine("Return only the JSON object. Do not add any leading or trailing text.");
            sb.AppendLine(request.UserPrompt);
        }
        else
        {
            sb.AppendLine("Task: generate a concise description.");
            sb.AppendLine(request.UserPrompt);
        }

        return sb.ToString();
    }

    private static LocalLlmResponse ParseTagResponse(string modelName, string output)
    {
        var tags = new List<string>();
        var confidence = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        string? notes = null;

        if (TryParseJsonObject(output, out var doc))
        {
            using (doc)
            {
                if (doc.RootElement.TryGetProperty("tags", out var tagsEl) && tagsEl.ValueKind == JsonValueKind.Array)
                {
                    foreach (var item in tagsEl.EnumerateArray())
                    {
                        if (item.ValueKind == JsonValueKind.String)
                        {
                            var value = item.GetString();
                            if (!string.IsNullOrWhiteSpace(value))
                                tags.Add(value.Trim());
                        }
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
            Provider = "internal",
            Model = modelName,
            Tags = tags,
            Confidence = confidence,
            Notes = notes,
            RawResponse = output,
            Latency = TimeSpan.Zero,
        };
    }

    private static LocalLlmResponse ParseDescriptionResponse(string modelName, string output)
    {
        string? description = null;
        var confidence = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);

        if (TryParseJsonObject(output, out var doc))
        {
            using (doc)
            {
                if (doc.RootElement.TryGetProperty("description", out var descEl) && descEl.ValueKind == JsonValueKind.String)
                    description = descEl.GetString();

                if (doc.RootElement.TryGetProperty("confidence", out var confEl) && confEl.ValueKind == JsonValueKind.Number && confEl.TryGetSingle(out var score))
                    confidence["description"] = score;
            }
        }

        description ??= output.Trim();
        return new LocalLlmResponse
        {
            Provider = "internal",
            Model = modelName,
            Description = description,
            Confidence = confidence,
            RawResponse = output,
            Latency = TimeSpan.Zero,
        };
    }

    private static bool TryParseJsonObject(string raw, out JsonDocument doc)
    {
        try
        {
            var start = raw.IndexOf('{');
            var end = raw.LastIndexOf('}');
            var json = start >= 0 && end > start ? raw[start..(end + 1)] : raw;
            doc = JsonDocument.Parse(json);
            return true;
        }
        catch
        {
            doc = null!;
            return false;
        }
    }

    private static ModelParams CreateModelParams(string modelPath, int gpuLayerCount)
    {
        return new ModelParams(modelPath)
        {
            ContextSize = 4096,
            GpuLayerCount = Math.Max(0, gpuLayerCount),
        };
    }

    internal static int ResolveCompletionTokenLimit(int requestedMaxOutputTokens) =>
        LocalLlmGenerationDefaults.ResolveInternalOutputTokenLimit(requestedMaxOutputTokens);

    private static InferenceParams CreateInferenceParams(int requestedMaxOutputTokens)
    {
        return new InferenceParams
        {
            MaxTokens = ResolveCompletionTokenLimit(requestedMaxOutputTokens),
            AntiPrompts = ["User:", "</s>"],
            SamplingPipeline = CreateSamplingPipeline(),
        };
    }

    internal static DefaultSamplingPipeline CreateSamplingPipeline()
    {
        return new DefaultSamplingPipeline
        {
            Temperature = LocalLlmGenerationDefaults.Temperature,
            TopP = LocalLlmGenerationDefaults.TopP,
            TopK = LocalLlmGenerationDefaults.TopK,
            RepeatPenalty = LocalLlmGenerationDefaults.RepeatPenalty,
        };
    }
}
