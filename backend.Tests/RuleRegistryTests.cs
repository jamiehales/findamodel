using System.Text.Json;
using findamodel.Services.Rules;
using Xunit;

namespace findamodel.Tests;

public class RuleRegistryTests
{
    // ── DeserializeRules ──────────────────────────────────────────────────────

    [Fact]
    public void DeserializeRules_NullYaml_ReturnsEmpty()
    {
        var result = RuleRegistry.DeserializeRules(null);
        Assert.Empty(result);
    }

    [Fact]
    public void DeserializeRules_EmptyYaml_ReturnsEmpty()
    {
        var result = RuleRegistry.DeserializeRules("");
        Assert.Empty(result);
    }

    [Fact]
    public void DeserializeRules_InvalidYaml_ReturnsEmpty()
    {
        var result = RuleRegistry.DeserializeRules("{ invalid yaml [[[");
        Assert.Empty(result);
    }

    [Fact]
    public void DeserializeRules_ValidYaml_ParsesFieldKeys()
    {
        const string yaml = """
            creator:
                            source: folder
                            expression: "^([^/]+)"
            collection:
              rule: regex
              source: folder
              expression: "^([^/]+)"
            """;
        var result = RuleRegistry.DeserializeRules(yaml);
        Assert.True(result.ContainsKey("creator"));
        Assert.True(result.ContainsKey("collection"));
    }

    [Fact]
    public void DeserializeRules_ParsedElement_ContainsRuleProperty()
    {
        const string yaml = "creator:\n  rule: regex\n";
        var result = RuleRegistry.DeserializeRules(yaml);
        Assert.True(result["creator"].TryGetProperty("rule", out var ruleEl));
        Assert.Equal("regex", ruleEl.GetString());
    }

    [Fact]
    public void DeserializeRules_WithWhitespaceOnlyYaml_ReturnsEmpty()
    {
        var result = RuleRegistry.DeserializeRules("   \n\n ");
        Assert.Empty(result);
    }

    // ── Evaluate ──────────────────────────────────────────────────────────────

    [Fact]
    public void Evaluate_UnknownRuleName_ReturnsNull()
    {
        var config = JsonDocument.Parse("""{"rule":"unknowntype"}""").RootElement;
        var result = RuleRegistry.Evaluate("creator", "/model.stl", [], config);
        Assert.Null(result);
    }

    [Fact]
    public void Evaluate_MissingRuleProperty_ReturnsNull()
    {
        var config = JsonDocument.Parse("""{"source":"folder"}""").RootElement;
        var result = RuleRegistry.Evaluate("creator", "/model.stl", [], config);
        Assert.Null(result);
    }

    [Fact]
    public void Evaluate_MissingRuleProperty_DefaultsToRegex()
    {
        var config = JsonDocument.Parse("""{"source":"folder","expression":"^/?([^/]+)"}""").RootElement;
        var result = RuleRegistry.Evaluate("model_name", "/models/dragon.stl", [], config);
        Assert.Equal("models", result);
    }

    [Fact]
    public void Evaluate_MissingSource_DefaultsToFullPath()
    {
        var config = JsonDocument.Parse("""{"rule":"regex","expression":"Fantasy"}""").RootElement;
        var result = RuleRegistry.Evaluate("creator", "/models/Fantasy/warrior.stl", [], config);
        Assert.Equal("Fantasy", result);
    }

    [Fact]
    public void Evaluate_RegexRule_DelegatesToRegexParser()
    {
        var config = JsonDocument.Parse("""{"rule":"regex","source":"folder","expression":"^([^/]+)"}""").RootElement;
        var result = RuleRegistry.Evaluate("creator", "Fantasy/Elves/warrior.stl", [], config);
        Assert.Equal("Fantasy", result);
    }

    [Fact]
    public void Evaluate_ExplicitFilenameRule_ReturnsNull()
    {
        var config = JsonDocument.Parse("""{"rule":"filename"}""").RootElement;
        var result = RuleRegistry.Evaluate("model_name", "/models/dragon.stl", [], config);
        Assert.Null(result);
    }

    [Fact]
    public void Evaluate_RegexBoolRule_ReturnsTrueOrFalseString()
    {
        var config = JsonDocument.Parse("""{"rule":"regex","expression":"supported"}""").RootElement;
        var trueResult = RuleRegistry.Evaluate("supported", "/models/supported_dragon.stl", [], config, RuleFieldType.Bool);
        var falseResult = RuleRegistry.Evaluate("supported", "/models/dragon.stl", [], config, RuleFieldType.Bool);
        Assert.Equal("true", trueResult);
        Assert.Equal("false", falseResult);
    }

    [Fact]
    public void Evaluate_RegexEnumRule_ReturnsMatchedKey()
    {
        var config = JsonDocument.Parse("""
            {
              "rule": "regex",
              "values": { "Fantasy": "fantasy", "SciFi": "scifi" }
            }
            """).RootElement;
        var result = RuleRegistry.Evaluate("category", "/models/fantasy/elf.stl", [], config, RuleFieldType.Enum);
        Assert.Equal("Fantasy", result);
    }
}
