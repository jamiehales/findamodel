using System.Text.Json;
using findamodel.Services.Rules;
using Xunit;

namespace findamodel.Tests;

public class FilenameRuleParserTests
{
    private static Dictionary<string, string?> NoFields => [];

    private static Dictionary<string, JsonElement> Options(bool includeExtension) =>
        new(StringComparer.OrdinalIgnoreCase)
        {
            ["include_extension"] = JsonDocument.Parse(includeExtension ? "true" : "false").RootElement,
        };

    private static Dictionary<string, JsonElement> EmptyOptions => [];

    [Fact]
    public void ParseValue_DefaultOptions_ReturnsNameWithoutExtension()
    {
        // CultureInfo.TextInfo.ToTitleCase treats underscores as word boundaries on Windows
        var result = FilenameRuleParser.ParseValue("/models/fantasy/elf_warrior.stl", NoFields, EmptyOptions);
        Assert.NotNull(result);
        Assert.DoesNotContain(".stl", result!, StringComparison.OrdinalIgnoreCase);
        Assert.StartsWith("E", result); // "elf" → "Elf" at minimum
    }

    [Fact]
    public void ParseValue_IncludeExtensionFalse_OmitsExtension()
    {
        var result = FilenameRuleParser.ParseValue("/models/dragon.obj", NoFields, Options(false));
        Assert.Equal("Dragon", result);
    }

    [Fact]
    public void ParseValue_IncludeExtensionTrue_RetainsExtension()
    {
        var result = FilenameRuleParser.ParseValue("/models/dragon.obj", NoFields, Options(true));
        // Extension is also title-cased: .obj → .Obj
        Assert.NotNull(result);
        Assert.StartsWith("Dragon", result);
        Assert.Contains("obj", result!, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ParseValue_TitleCasesFirstLetter()
    {
        var result = FilenameRuleParser.ParseValue("/models/dragon.stl", NoFields, EmptyOptions);
        Assert.Equal("Dragon", result);
    }

    [Fact]
    public void ParseValue_WindowsPathSeparator_ExtractsFilename()
    {
        var result = FilenameRuleParser.ParseValue(@"C:\models\subfolder\orc.stl", NoFields, EmptyOptions);
        Assert.Equal("Orc", result);
    }

    [Fact]
    public void ParseValue_FileWithNoExtension_ReturnsFullFilename()
    {
        // When filePath has no extension, GetFileNameWithoutExtension == GetFileName
        var result = FilenameRuleParser.ParseValue("/models/readme", NoFields, EmptyOptions);
        Assert.Equal("Readme", result);
    }

    [Fact]
    public void ParseValue_IncludeExtension_StringTrue_IncludesExtension()
    {
        // include_extension can be a string "true" (case-insensitive)
        var options = new Dictionary<string, JsonElement>(StringComparer.OrdinalIgnoreCase)
        {
            ["include_extension"] = JsonDocument.Parse("\"true\"").RootElement,
        };
        var result = FilenameRuleParser.ParseValue("/models/dragon.stl", NoFields, options);
        Assert.NotNull(result);
        Assert.Contains(".stl", result, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ParseValue_IncludeExtension_StringFalse_OmitsExtension()
    {
        var options = new Dictionary<string, JsonElement>(StringComparer.OrdinalIgnoreCase)
        {
            ["include_extension"] = JsonDocument.Parse("\"false\"").RootElement,
        };
        var result = FilenameRuleParser.ParseValue("/models/dragon.stl", NoFields, options);
        Assert.NotNull(result);
        Assert.DoesNotContain("stl", result, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ParseValue_IgnoresAvailableFieldsParameter()
    {
        // The filename rule does not use availableFields; should work with any value
        var fields = new Dictionary<string, string?> { ["creator"] = "Alice", ["collection"] = "Fantasy" };
        var result = FilenameRuleParser.ParseValue("/models/knight.stl", fields, EmptyOptions);
        Assert.Equal("Knight", result);
    }
}
