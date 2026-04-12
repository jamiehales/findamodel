using findamodel.Data.Entities;
using findamodel.Models;
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

    /// <summary>
    /// Descriptor for a field that exists in the per-model metadata YAML section.
    /// YamlKey is the snake_case key used in findamodel.yaml model_metadata entries.
    /// ApiKey is the camelCase key used in the API DTOs.
    /// </summary>
    internal sealed record ModelMetadataFieldDef(
        string ApiKey,
        string YamlKey,
        bool IsEnum = false,
        bool IsBool = false,
        bool IsFloat = false);

    internal static readonly ModelMetadataFieldDef[] ModelMetadataFields =
    [
        new("name",          "name"),
        new("partName",      "part_name"),
        new("creator",       "creator"),
        new("collection",    "collection"),
        new("subcollection", "subcollection"),
        new("tags",          "tags"),
        new("category",      "category",      IsEnum: true),
        new("type",          "type",          IsEnum: true),
        new("material",      "material",      IsEnum: true),
        new("supported",     "supported",     IsBool: true),
        new("raftHeightMm",  "raft_height_mm", IsFloat: true),
    ];

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
        new(
            Key: "part_name",
            RuleFieldType: RuleFieldType.String,
            GetRawValue: record => record.RawPartName,
            SetResolvedValue: (record, value) => record.PartName = value as string,
            GetResolvedValue: record => record.PartName),
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

    // ---- Model metadata helpers ----

    /// <summary>
    /// Parses a raw YAML entry dict (from the model_metadata section) into a <see cref="ModelMetadataEntry"/>.
    /// Returns null if the entry contains no recognisable values.
    /// </summary>
    internal static ModelMetadataEntry? ParseModelMetadataEntry(Dictionary<object, object> entryDict)
    {
        var d = entryDict.ToDictionary(
            kv => kv.Key?.ToString() ?? "",
            kv => kv.Value,
            StringComparer.OrdinalIgnoreCase);

        string? GetString(string yaml) => d.TryGetValue(yaml, out var v) ? v?.ToString() : null;
        bool? GetBool(string yaml)
        {
            if (!d.TryGetValue(yaml, out var v)) return null;
            return v switch
            {
                bool b => b,
                string s when bool.TryParse(s, out var p) => p,
                _ => null
            };
        }
        float? GetFloat(string yaml)
        {
            if (!d.TryGetValue(yaml, out var v)) return null;
            return v switch
            {
                float f => f,
                double dbl => (float)dbl,
                int i => (float)i,
                string s when float.TryParse(s, System.Globalization.NumberStyles.Float,
                    System.Globalization.CultureInfo.InvariantCulture, out var p) => p,
                _ => null
            };
        }
        List<string>? GetStringList(string yaml)
        {
            if (!d.TryGetValue(yaml, out var v)) return null;
            if (v is not List<object> list) return null;
            return TagListHelper.Normalize(list.Select(x => x?.ToString() ?? string.Empty));
        }

        var name = GetString("name");
        var partName = GetString("part_name");
        var creator = GetString("creator");
        var collection = GetString("collection");
        var subcollection = GetString("subcollection");
        var tags = GetStringList("tags");
        var category = GetString("category");
        var type = GetString("type");
        var material = GetString("material");
        var supported = GetBool("supported");
        var raftHeightMm = GetFloat("raft_height_mm");

        if (name == null && partName == null && creator == null && collection == null &&
            subcollection == null && tags == null && category == null && type == null && material == null &&
            supported == null && raftHeightMm == null)
            return null;

        return new ModelMetadataEntry(
            name, partName, creator, collection, subcollection, tags, category, type, material,
            supported, raftHeightMm);
    }

    /// <summary>
    /// Serialises a <see cref="ModelMetadataEntry"/> to a dictionary suitable for YAML output.
    /// Null / empty fields are omitted.
    /// </summary>
    internal static Dictionary<object, object> ToYamlDictionary(ModelMetadataEntry entry)
    {
        var d = new Dictionary<object, object>();
        if (entry.Name != null) d["name"] = entry.Name;
        if (entry.PartName != null) d["part_name"] = entry.PartName;
        if (entry.Creator != null) d["creator"] = entry.Creator;
        if (entry.Collection != null) d["collection"] = entry.Collection;
        if (entry.Subcollection != null) d["subcollection"] = entry.Subcollection;
        if (entry.Tags != null) d["tags"] = TagListHelper.Normalize(entry.Tags);
        if (entry.Category != null) d["category"] = entry.Category;
        if (entry.Type != null) d["type"] = entry.Type;
        if (entry.Material != null) d["material"] = entry.Material;
        if (entry.Supported.HasValue) d["supported"] = entry.Supported.Value;
        if (entry.RaftHeightMm.HasValue) d["raft_height_mm"] = entry.RaftHeightMm.Value;
        return d;
    }

    /// <summary>Returns true when all fields are null / unset.</summary>
    internal static bool IsEmptyModelMetadataEntry(ModelMetadataEntry entry) =>
        entry.Name == null && entry.PartName == null && entry.Creator == null &&
        entry.Collection == null && entry.Subcollection == null && entry.Tags == null && entry.Category == null &&
        entry.Type == null && entry.Material == null && entry.Supported == null &&
        entry.RaftHeightMm == null;
}
