using findamodel.Data.Entities;
using findamodel.Services.Rules;

namespace findamodel.Services;

internal static class MetadataFieldRegistry
{
    internal sealed record MetadataFieldDefinition(
        string Key,
        RuleFieldType RuleFieldType,
        Func<DirectoryConfig, object?> GetRawValue,
        Action<DirectoryConfig, object?> SetResolvedValue,
        Func<DirectoryConfig, object?> GetResolvedValue);

    public static readonly MetadataFieldDefinition[] Definitions =
    [
        new(
            Key: "creator",
            RuleFieldType: RuleFieldType.String,
            GetRawValue: record => record.RawCreator,
            SetResolvedValue: (record, value) => record.Creator = value as string,
            GetResolvedValue: record => record.Creator),
        new(
            Key: "collection",
            RuleFieldType: RuleFieldType.String,
            GetRawValue: record => record.RawCollection,
            SetResolvedValue: (record, value) => record.Collection = value as string,
            GetResolvedValue: record => record.Collection),
        new(
            Key: "subcollection",
            RuleFieldType: RuleFieldType.String,
            GetRawValue: record => record.RawSubcollection,
            SetResolvedValue: (record, value) => record.Subcollection = value as string,
            GetResolvedValue: record => record.Subcollection),
        new(
            Key: "category",
            RuleFieldType: RuleFieldType.Enum,
            GetRawValue: record => record.RawCategory,
            SetResolvedValue: (record, value) => record.Category = value as string,
            GetResolvedValue: record => record.Category),
        new(
            Key: "type",
            RuleFieldType: RuleFieldType.Enum,
            GetRawValue: record => record.RawType,
            SetResolvedValue: (record, value) => record.Type = value as string,
            GetResolvedValue: record => record.Type),
        new(
            Key: "material",
            RuleFieldType: RuleFieldType.Enum,
            GetRawValue: record => record.RawMaterial,
            SetResolvedValue: (record, value) => record.Material = value as string,
            GetResolvedValue: record => record.Material),
        new(
            Key: "supported",
            RuleFieldType: RuleFieldType.Bool,
            GetRawValue: record => record.RawSupported,
            SetResolvedValue: (record, value) => record.Supported = value as bool?,
            GetResolvedValue: record => record.Supported),
        new(
            Key: "model_name",
            RuleFieldType: RuleFieldType.String,
            GetRawValue: record => record.RawModelName,
            SetResolvedValue: (record, value) => record.ModelName = value as string,
            GetResolvedValue: record => record.ModelName),
    ];

    private static readonly Dictionary<string, MetadataFieldDefinition> DefinitionsByKey =
        Definitions.ToDictionary(def => def.Key, StringComparer.OrdinalIgnoreCase);

    public static IEnumerable<string> Keys => Definitions.Select(def => def.Key);

    public static bool TryGet(string key, out MetadataFieldDefinition definition) =>
        DefinitionsByKey.TryGetValue(key, out definition!);

    public static RuleFieldType GetRuleFieldType(string key) =>
        TryGet(key, out var definition) ? definition.RuleFieldType : RuleFieldType.String;

    public static string? ValidateEnumValue(string key, string? value)
    {
        _ = key;
        return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
    }
}
