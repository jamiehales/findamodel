using System.Text.Json;
using System.Text.RegularExpressions;

namespace findamodel.Services.Rules;

/// <summary>
/// Rule: "regex" — applies a regex expression to a value derived from the file path.
/// Options:
///   source: full_path | folder | filename  (default: full_path)
///   expression: a regex pattern, or a sed-style substitution (s|pattern|replacement|flags).
///               Sed-style: returns the substituted string.
///               Plain regex: returns the first capture group, or the full match if no groups.
///               Bool field: returns "true" if the regex matches, "false" otherwise.
///   values: { EnumValue1: "regex1", EnumValue2: "regex2", ... }
///           Enum field: tries each regex in order; returns the key of the first match.
///           Mutually exclusive with expression.
/// The fieldType parameter (supplied by the caller, not from config) controls bool/enum behaviour.
/// </summary>
public static class RegexRuleParser
{
    // Matches sed-style substitution: s<delim><pattern><delim><replacement><delim>[flags]
    private static readonly Regex SedPattern = new(@"^s(.)(.+?)\1(.*?)\1([gimsxy]*)$", RegexOptions.Compiled | RegexOptions.IgnoreCase);

    public static string? ParseValue(
        string filePath,
        RuleFieldType fieldType,
        Dictionary<string, JsonElement> options)
    {
        var source = options.TryGetValue("source", out var sv) ? sv.GetString() : "full_path";
        var input = source?.ToLowerInvariant() switch
        {
            "folder"    => Path.GetDirectoryName(filePath.Replace('\\', '/'))?.Replace('\\', '/'),
            "filename"  => Path.GetFileName(filePath),
            _           => filePath.Replace('\\', '/'),  // full_path (default)
        };
        if (input is null) return null;

        RegexOptions regexOptions = RegexOptions.IgnoreCase | RegexOptions.CultureInvariant;

        // Enum mode: values map takes precedence over expression
        if (fieldType == RuleFieldType.Enum
            && options.TryGetValue("values", out var valuesEl)
            && valuesEl.ValueKind == JsonValueKind.Object)
        {
            foreach (var prop in valuesEl.EnumerateObject())
            {
                var pattern = prop.Value.GetString();
                if (string.IsNullOrEmpty(pattern)) continue;
                if (Regex.IsMatch(input, pattern)) return prop.Name;
            }
            return null;
        }

        if (!options.TryGetValue("expression", out var exprEl)) return null;
        var expression = exprEl.GetString();
        if (string.IsNullOrEmpty(expression)) return null;

        var sedMatch = SedPattern.Match(expression);
        if (sedMatch.Success)
        {
            var pattern     = sedMatch.Groups[2].Value;
            var replacement = ConvertSedReplacement(sedMatch.Groups[3].Value);
            var flagsStr    = sedMatch.Groups[4].Value;
            var sedFlags = ParseFlags(flagsStr);
            regexOptions = sedFlags | regexOptions;
            return Regex.Replace(input, pattern, replacement, regexOptions);
        }

        // Boolean mode: match = "true", no match = "false"
        if (fieldType == RuleFieldType.Bool)
            return Regex.IsMatch(input, expression, regexOptions) ? "true" : "false";

        // Plain regex: return first capture group, or full match
        var m = Regex.Match(input, expression, regexOptions);
        if (!m.Success) return null;
        return m.Groups.Count > 1 ? m.Groups[1].Value : m.Value;
    }

    /// <summary>Converts sed backreferences (\1, \2, …) to .NET replacement syntax ($1, $2, …).</summary>
    private static string ConvertSedReplacement(string sedReplacement) =>
        Regex.Replace(sedReplacement, @"\\(\d+)", @"$$$1");

    private static RegexOptions ParseFlags(string flags)
    {
        var opts = RegexOptions.None;
        if (flags.Contains('i')) opts |= RegexOptions.IgnoreCase;
        if (flags.Contains('m')) opts |= RegexOptions.Multiline;
        if (flags.Contains('s')) opts |= RegexOptions.Singleline;
        if (flags.Contains('x')) opts |= RegexOptions.IgnorePatternWhitespace;
        return opts;
    }
}
