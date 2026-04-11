using System.Security.Cryptography;
using System.Text.Json;
using YamlDotNet.Serialization;
using findamodel.Models;

namespace findamodel.Services;

internal sealed record RawConfigFields(
    string? Creator,
    string? Collection,
    string? Subcollection,
    string? Category,
    string? Type,
    string? Material,
    bool? Supported,
    float? RaftHeightMm,
    string? ModelName = null,
    string? RulesYaml = null);

/// <summary>
/// Handles reading and writing findamodel.yaml config files, including
/// YAML parsing, file hash computation, and rule extraction.
/// </summary>
public sealed class DirectoryConfigReader(ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Config);
    internal const string ConfigFileName = "findamodel.yaml";

    internal static readonly IDeserializer YamlDeserializer = new DeserializerBuilder().Build();
    internal static readonly ISerializer YamlSerializer = new SerializerBuilder()
        .ConfigureDefaultValuesHandling(DefaultValuesHandling.OmitNull)
        .Build();

    internal static async Task<string> ComputeFileHashAsync(string filePath)
    {
        await using var stream = File.OpenRead(filePath);
        var hash = await SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLower();
    }

    internal async Task<RawConfigFields?> ParseConfigFileAsync(string filePath)
    {
        try
        {
            using var reader = new StreamReader(filePath);
            var yaml = await reader.ReadToEndAsync();
            var parsed = YamlDeserializer.Deserialize<Dictionary<string, object>>(yaml) ?? new Dictionary<string, object>();

            return new RawConfigFields(
                Creator: TryGetString(parsed, "creator"),
                Collection: TryGetString(parsed, "collection"),
                Subcollection: TryGetString(parsed, "subcollection"),
                Category: MetadataFieldRegistry.ValidateEnumValue("category", TryGetString(parsed, "category")),
                Type: MetadataFieldRegistry.ValidateEnumValue("type", TryGetString(parsed, "type")),
                Material: MetadataFieldRegistry.ValidateEnumValue("material", TryGetString(parsed, "material")),
                Supported: TryGetBool(parsed, "supported"),
                RaftHeightMm: TryGetFloat(parsed, "raftHeight"),
                ModelName: TryGetString(parsed, "model_name"),
                RulesYaml: ExtractRulesYaml(parsed)
            );
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to parse config file '{FilePath}'; treating as empty", filePath);
            return null;
        }
    }

    internal async Task WriteConfigFileAsync(string rootPath, string dirPath, UpdateDirectoryConfigRequest req)
    {
        var fullDirPath = ConfigInheritanceResolver.GetFullDirPath(rootPath, dirPath);
        var configPath = Path.Combine(fullDirPath, ConfigFileName);

        var data = new Dictionary<string, object>();
        if (req.ModelName != null) data["model_name"] = req.ModelName;
        if (req.Creator != null) data["creator"] = req.Creator;
        if (req.Collection != null) data["collection"] = req.Collection;
        if (req.Subcollection != null) data["subcollection"] = req.Subcollection;
        if (req.Category != null) data["category"] = req.Category;
        if (req.Type != null) data["type"] = req.Type;
        if (req.Material != null) data["material"] = req.Material;
        if (req.Supported.HasValue) data["supported"] = req.Supported.Value;
        if (req.RaftHeightMm.HasValue) data["raftHeight"] = req.RaftHeightMm.Value;

        if (req.FieldRules != null)
        {
            foreach (var (fieldName, ruleYaml) in req.FieldRules)
            {
                if (data.ContainsKey(fieldName)) continue;
                if (string.IsNullOrWhiteSpace(ruleYaml)) continue;
                try
                {
                    var ruleObj = YamlDeserializer.Deserialize<Dictionary<string, object>>(ruleYaml);
                    if (ruleObj != null && ruleObj.ContainsKey("rule"))
                        data[fieldName] = ruleObj;
                }
                catch (Exception ex)
                {
                    logger.LogWarning(ex, "Invalid rule YAML for field '{Field}'; skipping", fieldName);
                }
            }
        }
        else
        {
            // Legacy path: preserve rule definitions from the current YAML for fields not overridden by a plain value.
            var existingRules = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            if (File.Exists(configPath))
            {
                try
                {
                    var existingYaml = await File.ReadAllTextAsync(configPath);
                    var existing = YamlDeserializer.Deserialize<Dictionary<string, object>>(existingYaml);
                    if (existing != null)
                    {
                        var ruleFieldNames = MetadataFieldRegistry.Keys.ToArray();
                        foreach (var kvp in existing)
                        {
                            var fieldName = ruleFieldNames.FirstOrDefault(f =>
                                string.Equals(f, kvp.Key, StringComparison.OrdinalIgnoreCase));
                            if (fieldName == null) continue;

                            Dictionary<string, object>? ruleObj = kvp.Value switch
                            {
                                Dictionary<string, object> d => d,
                                Dictionary<object, object> d2 => d2
                                    .Where(kv => kv.Key != null)
                                    .ToDictionary(kv => kv.Key.ToString()!, kv => kv.Value),
                                _ => null
                            };

                            if (ruleObj != null && ruleObj.ContainsKey("rule"))
                                existingRules[fieldName] = ruleObj;
                        }
                    }
                }
                catch (Exception ex)
                {
                    logger.LogWarning(ex, "Could not read existing rules from '{ConfigPath}'; rules may be lost", configPath);
                }
            }

            foreach (var (field, ruleObj) in existingRules)
                if (!data.ContainsKey(field))
                    data[field] = ruleObj;
        }

        if (data.Count == 0)
        {
            if (File.Exists(configPath)) File.Delete(configPath);
            return;
        }

        var yaml = YamlSerializer.Serialize(data);
        await File.WriteAllTextAsync(configPath, yaml);
    }

    /// <summary>
    /// Serializes a resolved rules dictionary (JsonElement values) to YAML for storage.
    /// </summary>
    internal static string SerializeRulesToYaml(Dictionary<string, JsonElement> rules)
    {
        var plain = rules.ToDictionary(kvp => kvp.Key, kvp => JsonElementToYamlObject(kvp.Value));
        return YamlSerializer.Serialize(plain);
    }

    internal static object? JsonElementToYamlObject(JsonElement el) => el.ValueKind switch
    {
        JsonValueKind.Object => el.EnumerateObject()
            .ToDictionary(p => p.Name, p => JsonElementToYamlObject(p.Value)),
        JsonValueKind.Array => el.EnumerateArray().Select(JsonElementToYamlObject).ToList<object?>(),
        JsonValueKind.String => (object?)el.GetString(),
        JsonValueKind.Number when el.TryGetInt64(out var i) => (object?)i,
        JsonValueKind.Number => (object?)el.GetDouble(),
        JsonValueKind.True => (object?)true,
        JsonValueKind.False => (object?)false,
        _ => null
    };

    private static string? ExtractRulesYaml(Dictionary<string, object> data)
    {
        var ruleFieldNames = MetadataFieldRegistry.Keys.ToArray();
        var rules = new Dictionary<string, object>();

        foreach (var kvp in data)
        {
            var fieldName = ruleFieldNames.FirstOrDefault(f =>
                string.Equals(f, kvp.Key, StringComparison.OrdinalIgnoreCase));
            if (fieldName == null) continue;

            Dictionary<string, object>? ruleObj = kvp.Value switch
            {
                Dictionary<string, object> d => d,
                Dictionary<object, object> d2 => d2
                    .Where(kv => kv.Key != null)
                    .ToDictionary(kv => kv.Key.ToString()!, kv => kv.Value),
                _ => null
            };

            if (ruleObj == null || !ruleObj.ContainsKey("rule")) continue;
            rules[fieldName] = ruleObj;
        }

        return rules.Count > 0 ? YamlSerializer.Serialize(rules) : null;
    }

    private static string? TryGetString(Dictionary<string, object> data, string propertyName)
    {
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase)) continue;
            if (kvp.Value is Dictionary<object, object> || kvp.Value is Dictionary<string, object>) return null;
            var value = kvp.Value?.ToString();
            return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
        }
        return null;
    }

    private static bool? TryGetBool(Dictionary<string, object> data, string propertyName)
    {
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase)) continue;
            if (kvp.Value is Dictionary<object, object> || kvp.Value is Dictionary<string, object>) return null;
            if (kvp.Value is bool b) return b;
            if (kvp.Value is string s && bool.TryParse(s, out var parsed)) return parsed;
        }
        return null;
    }

    private static float? TryGetFloat(Dictionary<string, object> data, string propertyName)
    {
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase)) continue;
            if (kvp.Value is Dictionary<object, object> || kvp.Value is Dictionary<string, object>) return null;
            if (kvp.Value is float f && float.IsFinite(f)) return f;
            if (kvp.Value is double d && double.IsFinite(d)) return (float)d;
            if (kvp.Value is decimal m)
            {
                var asDouble = (double)m;
                if (asDouble <= float.MaxValue && asDouble >= float.MinValue)
                    return (float)asDouble;
            }
            if (kvp.Value is int i) return i;
            if (kvp.Value is long l && l <= float.MaxValue && l >= float.MinValue) return l;
            if (kvp.Value is string s
                && float.TryParse(s, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var parsed)
                && float.IsFinite(parsed))
                return parsed;
        }
        return null;
    }
}
