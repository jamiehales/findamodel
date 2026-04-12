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
        public List<string> Tags { get; init; } = [];
        public string? Category { get; init; }
        public string? Type { get; init; }
        public string? Material { get; init; }
        public bool? Supported { get; init; }
        public float? RaftHeightMm { get; init; }
        public string? ModelName { get; init; }
        public string? PartName { get; init; }
    }

    /// <summary>
    /// Evaluates resolved rules against <paramref name="fullFilePath"/> and merges them into the
    /// plain values provided. Returns the merged <see cref="ComputedMetadata"/> and, optionally,
    /// a map of field name → YAML snippet for every rule-derived field.
    /// </summary>
    internal static (ComputedMetadata Resolved, Dictionary<string, string>? RuleConfigs) ResolveForPath(
        ComputedMetadata plain,
        Dictionary<string, JsonElement> resolvedRules,
        string fullFilePath,
        bool collectRuleConfigs = false)
    {
        if (resolvedRules.Count == 0)
            return (plain, null);

        // Seed the working value map from plain fields
        var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase)
        {
            ["creator"] = plain.Creator,
            ["collection"] = plain.Collection,
            ["subcollection"] = plain.Subcollection,
            ["tags"] = plain.Tags,
            ["category"] = plain.Category,
            ["type"] = plain.Type,
            ["material"] = plain.Material,
            ["supported"] = plain.Supported,
            ["raftHeight"] = plain.RaftHeightMm,
            ["model_name"] = plain.ModelName,
            ["part_name"] = plain.PartName,
        };

        // Available (non-rule) fields passed to rule evaluators
        var available = new Dictionary<string, string?>(StringComparer.OrdinalIgnoreCase)
        {
            ["creator"] = plain.Creator,
            ["collection"] = plain.Collection,
            ["subcollection"] = plain.Subcollection,
            ["category"] = plain.Category,
            ["type"] = plain.Type,
            ["material"] = plain.Material,
            ["model_name"] = plain.ModelName,
            ["part_name"] = plain.PartName,
        };
        foreach (var field in resolvedRules.Keys) available.Remove(field);

        Dictionary<string, string>? ruleConfigs = collectRuleConfigs
            ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            : null;

        foreach (var (field, ruleEl) in resolvedRules)
        {
            var normalizedField = MetadataFieldRegistry.TryGet(field, out var fieldDef)
                ? fieldDef.Key
                : field.ToLowerInvariant();
            var ft = MetadataFieldRegistry.GetRuleFieldType(normalizedField);
            var value = RuleRegistry.Evaluate(field, fullFilePath, available, ruleEl, ft);
            if (value == null) continue;

            if (ruleConfigs != null)
                ruleConfigs[normalizedField] = RuleConfigToYamlSnippet(field, ruleEl);

            object? converted = ft switch
            {
                RuleFieldType.Bool => value.Equals("true", StringComparison.OrdinalIgnoreCase),
                RuleFieldType.Enum => MetadataFieldRegistry.ValidateEnumValue(normalizedField, value),
                _ => value,
            };
            values[normalizedField] = converted;
        }

        var resolved = new ComputedMetadata
        {
            Creator = values["creator"] as string,
            Collection = values["collection"] as string,
            Subcollection = values["subcollection"] as string,
            Tags = TagListHelper.Normalize((values["tags"] as List<string>) ?? []),
            Category = values["category"] as string,
            Type = values["type"] as string,
            Material = values["material"] as string,
            Supported = values["supported"] as bool?,
            RaftHeightMm = values["raftHeight"] as float?,
            ModelName = values["model_name"] as string,
            PartName = values["part_name"] as string,
        };

        return (resolved, ruleConfigs?.Count > 0 ? ruleConfigs : null);
    }

    /// <summary>
    /// Computes the folder-resolved metadata a model inherits from its directory config,
    /// WITHOUT applying any per-model overrides from the model_metadata section.
    /// </summary>
    public static ComputedMetadata ComputeInherited(string fullFilePath, DirectoryConfig? dirConfig)
    {
        if (dirConfig == null)
            return new ComputedMetadata();

        var plain = new ComputedMetadata
        {
            Creator = dirConfig.Creator,
            Collection = dirConfig.Collection,
            Subcollection = dirConfig.Subcollection,
            Tags = TagListHelper.FromJson(dirConfig.TagsJson),
            Category = dirConfig.Category,
            Type = dirConfig.Type,
            Material = dirConfig.Material,
            Supported = dirConfig.Supported,
            RaftHeightMm = dirConfig.RaftHeightMm,
            ModelName = dirConfig.ModelName,
            PartName = dirConfig.PartName,
        };

        var resolvedRules = RuleRegistry.DeserializeRules(dirConfig.ResolvedRulesYaml);
        return ResolveForPath(plain, resolvedRules, fullFilePath).Resolved;
    }

    public static ComputedMetadata Compute(string fullFilePath, DirectoryConfig? dirConfig)
    {
        var computed = ComputeInherited(fullFilePath, dirConfig);

        if (dirConfig == null)
            return computed;

        var configEntry = GetModelMetadataEntry(dirConfig, Path.GetFileName(fullFilePath));
        if (configEntry == null)
            return computed;

        if (configEntry.Name != null) computed = computed with { ModelName = configEntry.Name };
        if (configEntry.PartName != null) computed = computed with { PartName = configEntry.PartName };
        if (configEntry.Creator != null) computed = computed with { Creator = configEntry.Creator };
        if (configEntry.Collection != null) computed = computed with { Collection = configEntry.Collection };
        if (configEntry.Subcollection != null) computed = computed with { Subcollection = configEntry.Subcollection };
        if (configEntry.Tags != null) computed = computed with { Tags = TagListHelper.Merge(computed.Tags, configEntry.Tags) };
        if (configEntry.Category != null) computed = computed with { Category = configEntry.Category };
        if (configEntry.Type != null) computed = computed with { Type = configEntry.Type };
        if (configEntry.Material != null) computed = computed with { Material = configEntry.Material };
        if (configEntry.Supported.HasValue) computed = computed with { Supported = configEntry.Supported };
        if (configEntry.RaftHeightMm.HasValue) computed = computed with { RaftHeightMm = configEntry.RaftHeightMm };

        return computed;
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

    private static string RuleConfigToYamlSnippet(string fieldName, JsonElement ruleEl)
    {
        var sb = new System.Text.StringBuilder();
        sb.AppendLine($"{fieldName}:");
        foreach (var prop in ruleEl.EnumerateObject())
        {
            var val = prop.Value.ValueKind switch
            {
                JsonValueKind.String => prop.Value.GetString() ?? "",
                JsonValueKind.True => "true",
                JsonValueKind.False => "false",
                JsonValueKind.Number => prop.Value.GetRawText(),
                _ => prop.Value.GetRawText()
            };
            sb.AppendLine($"  {prop.Name}: {val}");
        }
        return sb.ToString().TrimEnd();
    }
}
