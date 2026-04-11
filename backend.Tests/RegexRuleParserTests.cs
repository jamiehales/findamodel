using System.Text.Json;
using findamodel.Services.Rules;
using Xunit;

namespace findamodel.Tests;

public class RegexRuleParserTests
{
    // ── Helpers ───────────────────────────────────────────────────────────────

    private static Dictionary<string, JsonElement> Opts(object obj)
    {
        var json = JsonSerializer.Serialize(obj);
        var doc = JsonDocument.Parse(json);
        var dict = new Dictionary<string, JsonElement>(StringComparer.OrdinalIgnoreCase);
        foreach (var prop in doc.RootElement.EnumerateObject())
            dict[prop.Name] = prop.Value;
        return dict;
    }

    private static string? Eval(
        string filePath,
        object options,
        RuleFieldType fieldType = RuleFieldType.String)
        => RegexRuleParser.ParseValue(filePath, fieldType, Opts(options));

    // ── Plain regex ───────────────────────────────────────────────────────────

    [Fact]
    public void PlainRegex_ReturnsFirstCaptureGroup()
    {
        var result = Eval(
            "/models/Fantasy/elf.stl",
            new { source = "full_path", expression = @"/(Fantasy|SciFi)/" });
        Assert.Equal("Fantasy", result);
    }

    [Fact]
    public void PlainRegex_ReturnsFullMatch_WhenNoCapturingGroup()
    {
        var result = Eval(
            "/models/dragon.stl",
            new { expression = @"dragon" });
        Assert.Equal("dragon", result);
    }

    [Fact]
    public void PlainRegex_ReturnsNull_WhenNoMatch()
    {
        var result = Eval(
            "/models/dragon.stl",
            new { expression = @"^unicorn" });
        Assert.Null(result);
    }

    [Fact]
    public void PlainRegex_IsCaseInsensitiveByDefault()
    {
        var result = Eval(
            "/models/Dragon.stl",
            new { expression = @"dragon" });
        Assert.Equal("Dragon", result);
    }

    // ── Source modes ──────────────────────────────────────────────────────────

    [Fact]
    public void FolderSource_UsesParentDirectoryOnly()
    {
        // full path: /models/Fantasy/Elves/warrior.stl
        // folder (parent dir of a file): /models/Fantasy/Elves
        var result = Eval(
            "/models/Fantasy/Elves/warrior.stl",
            new { source = "folder", expression = @"Elves" });
        Assert.Equal("Elves", result);
    }

    [Fact]
    public void FolderSource_DoesNotMatchFilename()
    {
        var result = Eval(
            "/models/Fantasy/warrior.stl",
            new { source = "folder", expression = @"warrior" });
        // "warrior" is part of filename, not folder
        Assert.Null(result);
    }

    [Fact]
    public void FilenameSource_UsesFilenameOnly()
    {
        var result = Eval(
            "/models/Fantasy/elf_warrior.stl",
            new { source = "filename", expression = @"^(elf)" });
        Assert.Equal("elf", result);
    }

    [Fact]
    public void FilenameSource_DoesNotMatchFolder()
    {
        var result = Eval(
            "/models/Fantasy/dragon.stl",
            new { source = "filename", expression = @"Fantasy" });
        Assert.Null(result);
    }

    [Fact]
    public void FullPathSource_MatchesEntirePath()
    {
        var result = Eval(
            "/models/Fantasy/warrior.stl",
            new { source = "full_path", expression = @"Fantasy" });
        Assert.Equal("Fantasy", result);
    }

    // ── Bool field type ───────────────────────────────────────────────────────

    [Fact]
    public void BoolField_ReturnsTrue_WhenPatternMatches()
    {
        var result = Eval(
            "/models/supported_dragon.stl",
            new { expression = @"supported" },
            RuleFieldType.Bool);
        Assert.Equal("true", result);
    }

    [Fact]
    public void BoolField_ReturnsFalse_WhenPatternDoesNotMatch()
    {
        var result = Eval(
            "/models/dragon.stl",
            new { expression = @"supported" },
            RuleFieldType.Bool);
        Assert.Equal("false", result);
    }

    // ── Enum values map ───────────────────────────────────────────────────────

    [Fact]
    public void EnumValues_ReturnsKeyOfFirstMatchingRegex()
    {
        var result = Eval(
            "/models/fantasy/elf.stl",
            new
            {
                values = new
                {
                    Fantasy = "fantasy",
                    SciFi = @"sci.?fi",
                }
            },
            RuleFieldType.Enum);
        Assert.Equal("Fantasy", result);
    }

    [Fact]
    public void EnumValues_ReturnsNull_WhenNoRegexMatches()
    {
        var result = Eval(
            "/models/historical/knight.stl",
            new
            {
                values = new
                {
                    Fantasy = "fantasy",
                    SciFi = @"sci.?fi",
                }
            },
            RuleFieldType.Enum);
        Assert.Null(result);
    }

    [Fact]
    public void EnumValues_FirstMatchWins_WhenMultipleMatch()
    {
        // Both patterns could match, but "Fantasy" comes first in the JSON object
        var result = Eval(
            "/models/fantasy-scifi/model.stl",
            new
            {
                values = new
                {
                    Fantasy = "fantasy",
                    SciFi = @"scifi",
                }
            },
            RuleFieldType.Enum);
        Assert.Equal("Fantasy", result);
    }

    // ── Sed substitution ──────────────────────────────────────────────────────

    [Fact]
    public void SedSubstitution_ReplacesDash_InSimpleFilename()
    {
        // .NET Regex.Replace replaces all matches (equivalent to sed with /g flag)
        var result = Eval(
            "fantasy-model.stl",
            new { expression = @"s/-/_/" });
        Assert.Equal("fantasy_model.stl", result);
    }

    [Fact]
    public void SedSubstitution_ReplacesAllMatches_WithGlobalFlag()
    {
        var result = Eval(
            "/fantasy-foo-bar.stl",
            new { expression = @"s|-|_|g" });
        Assert.Equal("/fantasy_foo_bar.stl", result);
    }

    [Fact]
    public void SedSubstitution_SupportsBackreferences()
    {
        // Use / as delimiter to avoid conflict with | in the capture group
        var result = Eval(
            "/models/Fantasy/elf.stl",
            new { source = "full_path", expression = @"s/.*(Fantasy|SciFi).*/\1/" });
        Assert.Equal("Fantasy", result);
    }

    [Fact]
    public void SedSubstitution_CaseInsensitiveFlag()
    {
        var result = Eval(
            "DRAGON.stl",
            new { source = "filename", expression = @"s|dragon|replaced|i" });
        Assert.Equal("replaced.stl", result);
    }

    // ── Missing expression ────────────────────────────────────────────────────

    [Fact]
    public void ParseValue_ReturnsNull_WhenNoExpressionOrValues()
    {
        var result = Eval("/models/dragon.stl", new { source = "full_path" });
        Assert.Null(result);
    }
}
