using System.Text.Json;
using YamlDotNet.Serialization;

namespace findamodel.Services.Rules;

public enum RuleFieldType { String, Bool, Enum }

/// <summary>
/// Registry that maps rule names to their parser implementations and evaluates rules for model files.
/// </summary>
public static class RuleRegistry
{
    /// <summary>
    /// Evaluates a single rule for a given model file.
    /// </summary>
    /// <param name="fieldName">The metadata field name (e.g. "creator").</param>
    /// <param name="filePath">Full path to the model file.</param>
    /// <param name="availableFields">Resolved non-rule field values available for use by rules.</param>
    /// <param name="ruleConfig">The rule config JSON element. If "rule" is omitted, "regex" is assumed.</param>
    /// <param name="fieldType">The expected type of the field; governs how regex results are interpreted.</param>
    /// <returns>The computed value, or null if the rule name is unknown or evaluation fails.</returns>
    public static string? Evaluate(
        string fieldName,
        string filePath,
        Dictionary<string, string?> availableFields,
        JsonElement ruleConfig,
        RuleFieldType fieldType = RuleFieldType.String)
    {
        var ruleName = "regex";
        if (ruleConfig.TryGetProperty("rule", out var ruleNameEl))
        {
            if (ruleNameEl.ValueKind != JsonValueKind.String) return null;
            var configuredRuleName = ruleNameEl.GetString();
            if (string.IsNullOrWhiteSpace(configuredRuleName)) return null;
            ruleName = configuredRuleName;
        }

        // Collect options (all properties except "rule")
        var options = new Dictionary<string, JsonElement>(StringComparer.OrdinalIgnoreCase);
        foreach (var prop in ruleConfig.EnumerateObject())
        {
            if (!string.Equals(prop.Name, "rule", StringComparison.OrdinalIgnoreCase))
                options[prop.Name] = prop.Value;
        }

        return ruleName.ToLowerInvariant() switch
        {
            "regex" => RegexRuleParser.ParseValue(filePath, fieldType, options),
            _ => null
        };
    }

    private static readonly IDeserializer YamlDeserializer = new DeserializerBuilder().Build();

    /// <summary>
    /// Deserializes a rules YAML string into a dictionary of field name → rule config element.
    /// Returns an empty dictionary if the YAML is null/empty or cannot be parsed.
    /// </summary>
    public static Dictionary<string, JsonElement> DeserializeRules(string? yaml)
    {
        if (string.IsNullOrEmpty(yaml)) return new();
        try
        {
            var parsed = YamlDeserializer.Deserialize<Dictionary<string, object>>(yaml) ?? new();
            var json = JsonSerializer.Serialize(ConvertYamlObjectToSerializable(parsed));
            return JsonSerializer.Deserialize<Dictionary<string, JsonElement>>(json) ?? new();
        }
        catch { return new(); }
    }

    private static object? ConvertYamlObjectToSerializable(object? value) => value switch
    {
        Dictionary<object, object> d => d
            .Where(kv => kv.Key != null)
            .ToDictionary(kv => kv.Key.ToString()!, kv => ConvertYamlObjectToSerializable(kv.Value)),
        Dictionary<string, object> d => d
            .ToDictionary(kv => kv.Key, kv => ConvertYamlObjectToSerializable(kv.Value)),
        _ => value
    };
}
