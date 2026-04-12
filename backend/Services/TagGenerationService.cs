using System.Text.Json;
using System.Security.Cryptography;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using Microsoft.EntityFrameworkCore;

namespace findamodel.Services;

public class TagGenerationService(
    IDbContextFactory<ModelCacheContext> dbFactory,
    LocalLlmProviderResolver providerResolver,
    AppConfigService appConfigService,
    IConfiguration config,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger _logger = loggerFactory.CreateLogger(LogChannels.Llm);
    private readonly string _rendersPath = config["Cache:RendersPath"] ?? Path.Combine("data", "cache", "renders");
    internal const int CurrentTagGenerationPromptVersion = 2;
    internal const int CurrentLlmRuntimeVersion = 1;

    public async Task<GeneratedTagsResultDto?> GenerateForModelAsync(Guid modelId, CancellationToken ct)
    {
        await using var db = await dbFactory.CreateDbContextAsync(ct);
        var model = await db.Models.FirstOrDefaultAsync(m => m.Id == modelId, ct);
        if (model == null)
            return null;

        var appConfig = await appConfigService.GetAsync();
        if (!appConfig.TagGenerationEnabled)
        {
            _logger.LogDebug("Tag generation skipped for model {ModelId}: TagGenerationEnabled=false", model.Id);
            return await PersistFailure(model, "Tag generation is disabled in settings.", db, ct);
        }

        var schemaTags = await db.MetadataDictionaryValues
            .AsNoTracking()
            .Where(v => v.Field == "tags")
            .OrderBy(v => v.Value)
            .Select(v => v.Value)
            .ToListAsync(ct);

        var allowedTags = TagListHelper.Normalize(schemaTags);
        if (allowedTags.Count == 0)
        {
            _logger.LogDebug("Tag generation skipped for model {ModelId}: no configured schema tags", model.Id);
            return await PersistFailure(model, "No configured tag schema values exist in settings.", db, ct);
        }

        var expectedChecksum = ComputeGenerationChecksum(model, appConfig, allowedTags);
        _logger.LogDebug(
            "Tag generation starting for model {ModelId}: provider={Provider}, model={ModelName}, schemaCount={SchemaCount}, checksum={Checksum}",
            model.Id,
            appConfig.TagGenerationProvider,
            appConfig.TagGenerationModel,
            allowedTags.Count,
            expectedChecksum);

        model.GeneratedTagsStatus = "pending";
        model.GeneratedTagsError = null;
        await db.SaveChangesAsync(ct);

        var provider = providerResolver.Resolve(appConfig.TagGenerationProvider);
        var providerSettings = new LocalLlmProviderSettings(
            appConfig.TagGenerationEndpoint,
            appConfig.TagGenerationModel,
            appConfig.TagGenerationTimeoutMs);

        try
        {
            var request = BuildTagRequest(model, allowedTags, appConfig.TagGenerationMaxTags, ResolvePreviewPath(model));
            var response = await provider.GenerateAsync(providerSettings, request, ct);

            var filtered = FilterTags(response, allowedTags, appConfig.TagGenerationMinConfidence, appConfig.TagGenerationMaxTags);

            model.GeneratedTagsJson = TagListHelper.ToJsonOrNull(filtered.Tags);
            model.GeneratedTagsConfidenceJson = filtered.Confidence.Count == 0
                ? null
                : JsonSerializer.Serialize(filtered.Confidence);
            model.GeneratedTagsModel = response.Model;
            model.GeneratedTagsAt = DateTime.UtcNow;
            model.GeneratedTagsChecksum = expectedChecksum;
            model.GeneratedTagsStatus = "success";
            model.GeneratedTagsError = null;

            if (appConfig.TagGenerationAutoApply && filtered.Tags.Count > 0)
            {
                var merged = TagListHelper.Merge(TagListHelper.FromJson(model.CalculatedTagsJson), filtered.Tags);
                model.CalculatedTagsJson = TagListHelper.ToJsonOrNull(merged);
            }

            await db.SaveChangesAsync(ct);

            _logger.LogInformation(
                "Generated {TagCount} tags for model {ModelId} using provider {Provider} ({Model})",
                filtered.Tags.Count,
                model.Id,
                provider.Name,
                response.Model);

            return new GeneratedTagsResultDto(
                model.Id,
                model.GeneratedTagsStatus ?? "success",
                filtered.Tags,
                filtered.Confidence,
                null,
                model.GeneratedTagsAt,
                provider.Name,
                response.Model);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed generating tags for model {ModelId}", model.Id);
            return await PersistFailure(model, ex.Message, db, ct);
        }
    }

    public async Task<GeneratedTagsResultDto?> GetGeneratedTagsAsync(Guid modelId, CancellationToken ct)
    {
        await using var db = await dbFactory.CreateDbContextAsync(ct);
        var model = await db.Models.AsNoTracking().FirstOrDefaultAsync(m => m.Id == modelId, ct);
        if (model == null)
            return null;

        return ToResult(model);
    }

    public async Task<bool> ClearGeneratedTagsAsync(Guid modelId, CancellationToken ct)
    {
        await using var db = await dbFactory.CreateDbContextAsync(ct);
        var model = await db.Models.FirstOrDefaultAsync(m => m.Id == modelId, ct);
        if (model == null)
            return false;

        model.GeneratedTagsJson = null;
        model.GeneratedTagsConfidenceJson = null;
        model.GeneratedTagsModel = null;
        model.GeneratedTagsAt = DateTime.UtcNow;
        model.GeneratedTagsStatus = "none";
        model.GeneratedTagsError = null;

        await db.SaveChangesAsync(ct);
        return true;
    }

    private static LocalLlmRequest BuildTagRequest(CachedModel model, List<string> schemaTags, int maxTags, string? previewImagePath)
    {
        var context = new Dictionary<string, string?>(StringComparer.OrdinalIgnoreCase)
        {
            ["modelName"] = model.CalculatedModelName ?? Path.GetFileNameWithoutExtension(model.FileName),
            ["partName"] = model.CalculatedPartName,
            ["fileName"] = model.FileName,
            ["directory"] = model.Directory,
            ["category"] = model.CalculatedCategory,
            ["type"] = model.CalculatedType,
            ["material"] = model.CalculatedMaterial,
            ["creator"] = model.CalculatedCreator,
            ["collection"] = model.CalculatedCollection,
            ["subcollection"] = model.CalculatedSubcollection,
        };

        var systemPrompt =
            "You are a model-tagging assistant for tabletop 3D-print miniatures and terrain. " +
            "Return exactly one valid JSON object and nothing else. " +
            "Do not include markdown fences, the word json, prose, or explanation. " +
            "Your response must start with '{' and end with '}'.";
        var userPrompt =
            "Given the provided image and metadata context, return tags only from the allowed schema. " +
            "Focus on monochrome mesh renders (no color cues). " +
            $"Return at most {maxTags} tags as JSON: {{\"tags\":[...],\"confidence\":{{\"tag\":0.0}},\"notes\":\"optional\"}}. " +
            "Output the JSON object only with no leading or trailing text. " +
            $"Allowed tags: {string.Join(", ", schemaTags)}.";

        return new LocalLlmRequest
        {
            TaskKind = LocalLlmTaskKind.Tags,
            SystemPrompt = systemPrompt,
            UserPrompt = userPrompt,
            AllowedTags = schemaTags,
            Context = context,
            ImagePath = previewImagePath,
            MaxTags = maxTags,
            MaxOutputTokens = 512,
        };
    }

    private static (List<string> Tags, Dictionary<string, float> Confidence) FilterTags(
        LocalLlmResponse response,
        List<string> allowedTags,
        float minConfidence,
        int maxTags)
    {
        var allowedSet = new HashSet<string>(allowedTags, StringComparer.OrdinalIgnoreCase);
        var normalized = TagListHelper.Normalize(response.Tags);

        var result = new List<string>();
        var confidence = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        foreach (var tag in normalized)
        {
            if (!allowedSet.Contains(tag))
                continue;

            var score = response.Confidence.TryGetValue(tag, out var existingScore)
                ? existingScore
                : 0.5f;

            if (score < minConfidence)
                continue;

            result.Add(tag);
            confidence[tag] = score;
            if (result.Count >= maxTags)
                break;
        }

        return (result, confidence);
    }

    private string? ResolvePreviewPath(CachedModel model)
    {
        if (string.IsNullOrWhiteSpace(model.PreviewImagePath))
            return null;

        var fullPath = Path.Combine(_rendersPath, model.PreviewImagePath);
        return File.Exists(fullPath) ? fullPath : null;
    }

    private static Dictionary<string, float> ParseConfidence(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
            return new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);

        try
        {
            var parsed = JsonSerializer.Deserialize<Dictionary<string, float>>(json);
            return parsed ?? new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        }
        catch
        {
            return new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        }
    }

    private static GeneratedTagsResultDto ToResult(CachedModel model)
    {
        return new GeneratedTagsResultDto(
            model.Id,
            model.GeneratedTagsStatus ?? "none",
            TagListHelper.FromJson(model.GeneratedTagsJson),
            ParseConfidence(model.GeneratedTagsConfidenceJson),
            model.GeneratedTagsError,
            model.GeneratedTagsAt,
            null,
            model.GeneratedTagsModel);
    }

    internal static bool NeedsRegeneration(CachedModel model, AppConfigDto appConfig, IReadOnlyList<string> schemaTags)
    {
        if (!appConfig.TagGenerationEnabled)
            return false;

        if (schemaTags.Count == 0)
            return false;

        if (!string.Equals(model.GeneratedTagsStatus, "success", StringComparison.OrdinalIgnoreCase))
            return true;

        if (string.IsNullOrWhiteSpace(model.GeneratedTagsJson))
            return true;

        var expected = ComputeGenerationChecksum(model, appConfig, schemaTags);
        return !string.Equals(expected, model.GeneratedTagsChecksum, StringComparison.OrdinalIgnoreCase);
    }

    internal static string ComputeGenerationChecksum(CachedModel model, AppConfigDto appConfig, IReadOnlyList<string> schemaTags)
    {
        var normalizedSchema = schemaTags
            .Where(t => !string.IsNullOrWhiteSpace(t))
            .Select(t => t.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(t => t, StringComparer.OrdinalIgnoreCase);
        var schemaChecksumInput = string.Join(",", normalizedSchema);

        var payload = string.Join("|", [
            $"promptVersion:{CurrentTagGenerationPromptVersion}",
            $"llmVersion:{CurrentLlmRuntimeVersion}",
            $"model:{appConfig.TagGenerationModel}",
            $"checksum:{model.Checksum}",
            $"fileName:{model.FileName}",
            $"directory:{model.Directory}",
            $"schema:{schemaChecksumInput}",
        ]);

        var hash = SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(payload));
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    private static async Task<GeneratedTagsResultDto> PersistFailure(
        CachedModel model,
        string error,
        ModelCacheContext db,
        CancellationToken ct)
    {
        model.GeneratedTagsStatus = "failed";
        model.GeneratedTagsError = error.Length > 512 ? error[..512] : error;
        model.GeneratedTagsAt = DateTime.UtcNow;
        await db.SaveChangesAsync(ct);

        return ToResult(model);
    }
}
