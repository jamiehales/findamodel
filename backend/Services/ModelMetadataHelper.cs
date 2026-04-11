using System.Text.Json;
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
    }

    public static ComputedMetadata Compute(string fullFilePath, DirectoryConfig? dirConfig)
    {
        if (dirConfig == null)
            return new ComputedMetadata();

        var resolvedRules = RuleRegistry.DeserializeRules(dirConfig.ResolvedRulesYaml);
        var availableFields = new Dictionary<string, string?>();

        return new ComputedMetadata
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
}
