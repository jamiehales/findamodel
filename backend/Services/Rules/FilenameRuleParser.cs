using System.Text.Json;

namespace findamodel.Services.Rules;

/// <summary>
/// Rule: "filename" - sets the field value to the model file's name (without extension by default).
/// Options:
///   include_extension: true - include the file extension in the returned value.
/// </summary>
public static class FilenameRuleParser
{
    public static string? ParseValue(
        string filePath,
        Dictionary<string, string?> availableFields,
        Dictionary<string, JsonElement> options)
    {
        var includeExtension = options.TryGetValue("include_extension", out var v)
            && (v.ValueKind == JsonValueKind.True
                || (v.ValueKind == JsonValueKind.String
                    && string.Equals(v.GetString(), "true", StringComparison.OrdinalIgnoreCase)));

        var name = includeExtension
            ? Path.GetFileName(filePath)
            : Path.GetFileNameWithoutExtension(filePath);

        return name is null ? null : System.Globalization.CultureInfo.CurrentCulture.TextInfo.ToTitleCase(name.ToLower());
    }
}
