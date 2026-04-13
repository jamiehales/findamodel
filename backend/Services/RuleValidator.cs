using YamlDotNet.Serialization;

namespace findamodel.Services;

internal static class RuleValidator
{
    private static readonly IDeserializer YamlDeserializer = new DeserializerBuilder().Build();

    private static readonly HashSet<string> KnownRuleTypes =
        new(StringComparer.OrdinalIgnoreCase) { "regex" };

    /// <summary>
    /// Validates each field rule entry; returns a map of fieldName → error message for any failures.
    /// </summary>
    internal static Dictionary<string, string> ValidateRules(Dictionary<string, string> fieldRules)
    {
        var errors = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (fieldName, ruleYaml) in fieldRules)
        {
            if (string.IsNullOrWhiteSpace(ruleYaml)) continue;

            Dictionary<string, object>? ruleObj;
            try
            {
                ruleObj = YamlDeserializer.Deserialize<Dictionary<string, object>>(ruleYaml);
            }
            catch (Exception ex)
            {
                errors[fieldName] = $"Invalid YAML: {ex.Message}";
                continue;
            }

            if (ruleObj == null || ruleObj.Count == 0)
            {
                errors[fieldName] = "Invalid YAML: rule is empty";
                continue;
            }

            var ruleName = ruleObj.TryGetValue("rule", out var ruleNameObj) && ruleNameObj != null
                ? ruleNameObj.ToString() ?? string.Empty
                : "regex";
            if (!KnownRuleTypes.Contains(ruleName))
                errors[fieldName] = $"Unknown rule type \"{ruleName}\". Valid types: {string.Join(", ", KnownRuleTypes)}";
        }
        return errors;
    }
}
