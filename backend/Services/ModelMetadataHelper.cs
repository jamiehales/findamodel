using System.Text.Json;
using findamodel.Models;
using findamodel.Data.Entities;
using findamodel.Services.Rules;

namespace findamodel.Services;

/// <summary>
/// Computes per-model metadata by evaluating DirectoryConfig resolved values and rules
/// against a specific model file path.
/// </summary>
internal static class ModelMetadataHelper
{
    internal record ComputedMetadata
    {
        public string? Creator { get; init; }
        public string? Collection { get; init; }
        public string? Subcollection { get; init; }
        public string? Category { get; init; }
        public string? Type { get; init; }
        public string? Material { get; init; }
        public bool? Supported { get; init; }
        public string? ModelName { get; init; }
        public string? PartName { get; init; }
    }

    public static ComputedMetadata Compute(string fullFilePath, DirectoryConfig? dirConfig)
    {
        if (dirConfig == null)
            return new ComputedMetadata();

        var resolvedRules = RuleRegistry.DeserializeRules(dirConfig.ResolvedRulesYaml);
        var availableFields = new Dictionary<string, string?>();

        var computed = new ComputedMetadata
        {
            Creator = EvaluateString("creator", dirConfig.Creator, resolvedRules, fullFilePath, availableFields),
            Collection = EvaluateString("collection", dirConfig.Collection, resolvedRules, fullFilePath, availableFields),
            Subcollection = EvaluateString("subcollection", dirConfig.Subcollection, resolvedRules, fullFilePath, availableFields),
            Category = EvaluateString("category", dirConfig.Category, resolvedRules, fullFilePath, availableFields, RuleFieldType.Enum),
            Type = EvaluateString("type", dirConfig.Type, resolvedRules, fullFilePath, availableFields, RuleFieldType.Enum),
            Material = EvaluateString("material", dirConfig.Material, resolvedRules, fullFilePath, availableFields, RuleFieldType.Enum),
            Supported = EvaluateBool("supported", dirConfig.Supported, resolvedRules, fullFilePath, availableFields),
            ModelName = EvaluateString("model_name", dirConfig.ModelName, resolvedRules, fullFilePath, availableFields)
        };

        var configEntry = GetModelMetadataEntry(dirConfig, Path.GetFileName(fullFilePath));
        if (configEntry?.Name != null)
            computed = computed with { ModelName = configEntry.Name };
        if (configEntry?.PartName != null)
            computed = computed with { PartName = configEntry.PartName };
        if (configEntry?.Creator != null)
            computed = computed with { Creator = configEntry.Creator };
        if (configEntry?.Collection != null)
            computed = computed with { Collection = configEntry.Collection };
        if (configEntry?.Subcollection != null)
            computed = computed with { Subcollection = configEntry.Subcollection };
        if (configEntry?.Category != null)
            computed = computed with { Category = configEntry.Category };
        if (configEntry?.Type != null)
            computed = computed with { Type = configEntry.Type };
        if (configEntry?.Material != null)
            computed = computed with { Material = configEntry.Material };
        if (configEntry?.Supported.HasValue == true)
            computed = computed with { Supported = configEntry.Supported };

        return computed;
    }

    private static string? EvaluateString(
        string fieldName,
        string? plainValue,
        Dictionary<string, JsonElement> resolvedRules,
        string filePath,
        Dictionary<string, string?> availableFields,
        RuleFieldType fieldType = RuleFieldType.String)
    {
        if (resolvedRules.TryGetValue(fieldName, out var ruleEl))
            return RuleRegistry.Evaluate(fieldName, filePath, availableFields, ruleEl, fieldType);
        return plainValue;
    }

    private static bool? EvaluateBool(
        string fieldName,
        bool? plainValue,
        Dictionary<string, JsonElement> resolvedRules,
        string filePath,
        Dictionary<string, string?> availableFields)
    {
        if (!resolvedRules.TryGetValue(fieldName, out var ruleEl)) return plainValue;
        var result = RuleRegistry.Evaluate(fieldName, filePath, availableFields, ruleEl, RuleFieldType.Bool);
        return result != null && bool.TryParse(result, out var b) ? b : (bool?)null;
    }

    internal static ModelMetadataEntry? GetModelMetadataEntry(DirectoryConfig dirConfig, string fileName)
    {
        if (dirConfig.RawModelMetadataJson is null) return null;
        var dict = JsonSerializer.Deserialize<Dictionary<string, ModelMetadataEntry>>(
            dirConfig.RawModelMetadataJson,
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
        return dict?.FirstOrDefault(kv =>
            string.Equals(kv.Key, fileName, StringComparison.OrdinalIgnoreCase)).Value;
    }
}
