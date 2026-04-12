using System.Text.Json;

namespace findamodel.Services;

internal static class TagListHelper
{
    public const int MaxTagCount = 64;
    public const int MaxTagLength = 64;

    internal sealed record ValidationResult(List<string> Tags, string? Error);

    public static ValidationResult ValidateAndNormalize(IEnumerable<string>? tags)
    {
        if (tags == null)
            return new ValidationResult([], null);

        var normalized = Normalize(tags);
        return new ValidationResult(normalized, null);
    }

    public static List<string> Normalize(IEnumerable<string> tags)
    {
        var result = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var raw in tags)
        {
            if (result.Count >= MaxTagCount)
                break;

            var cleaned = NormalizeSingle(raw);
            if (cleaned == null)
                continue;
            if (cleaned.Length > MaxTagLength)
                cleaned = cleaned[..MaxTagLength].Trim();
            if (!seen.Add(cleaned))
                continue;
            result.Add(cleaned);
        }

        result.Sort(StringComparer.OrdinalIgnoreCase);
        return result;
    }

    public static List<string> Merge(params IEnumerable<string>?[] sources)
    {
        var combined = new List<string>();
        foreach (var source in sources)
        {
            if (source == null)
                continue;
            combined.AddRange(source);
        }
        return Normalize(combined);
    }

    public static string? ToJsonOrNull(IEnumerable<string>? tags)
    {
        if (tags == null)
            return null;

        var normalized = Normalize(tags);
        return normalized.Count == 0 ? null : JsonSerializer.Serialize(normalized);
    }

    public static List<string> FromJson(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
            return [];

        try
        {
            var parsed = JsonSerializer.Deserialize<List<string>>(json);
            return parsed == null ? [] : Normalize(parsed);
        }
        catch
        {
            return [];
        }
    }

    private static string? NormalizeSingle(string? input)
    {
        if (string.IsNullOrWhiteSpace(input))
            return null;

        var collapsed = string.Join(' ', input.Trim().Split(' ', StringSplitOptions.RemoveEmptyEntries));
        return string.IsNullOrWhiteSpace(collapsed) ? null : collapsed;
    }
}