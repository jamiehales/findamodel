using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class RuleValidatorTests
{
    [Fact]
    public void ValidateRules_EmptyInput_ReturnsNoErrors()
    {
        var errors = RuleValidator.ValidateRules(new Dictionary<string, string>());
        Assert.Empty(errors);
    }

    [Fact]
    public void ValidateRules_MissingRuleKey_DefaultsToRegex_AndReturnsNoErrors()
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "source: folder\nexpression: ^([^/]+)",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.Empty(errors);
    }

    [Fact]
    public void ValidateRules_ValidRegexRule_ReturnsNoErrors()
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "rule: regex\nsource: folder\nexpression: ^([^/]+)",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.Empty(errors);
    }

    [Fact]
    public void ValidateRules_ExplicitFilenameRule_ReturnsError()
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "rule: filename",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.True(errors.ContainsKey("creator"));
        Assert.Contains("filename", errors["creator"]);
    }

    [Fact]
    public void ValidateRules_UnknownRuleType_ReturnsError()
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "rule: magic\nsource: folder",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.True(errors.ContainsKey("creator"));
        Assert.Contains("magic", errors["creator"]);
    }

    [Fact]
    public void ValidateRules_InvalidYaml_ReturnsError()
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "{ this is: [invalid yaml",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.True(errors.ContainsKey("creator"));
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("\t")]
    public void ValidateRules_WhitespaceOrEmptyRule_IsSkipped(string ruleYaml)
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = ruleYaml,
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.Empty(errors);
    }

    [Fact]
    public void ValidateRules_MultipleFields_ValidatesIndependently()
    {
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "source: folder\nexpression: ^([^/]+)",
            ["collection"] = "source: folder\nexpression: ([^/]+)$",
            ["category"] = "rule: unknowntype",      // unknown rule type
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.False(errors.ContainsKey("creator"));
        Assert.False(errors.ContainsKey("collection"));
        Assert.True(errors.ContainsKey("category"));
    }

    [Fact]
    public void ValidateRules_RuleKeyIsCaseInsensitiveForFieldName()
    {
        // The errors dict is OrdinalIgnoreCase; querying with different case should find the error
        var rules = new Dictionary<string, string>
        {
            ["Creator"] = "source: folder",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.False(errors.ContainsKey("creator"));
    }

    [Fact]
    public void ValidateRules_EmptyRuleObject_ReturnsError()
    {
        // YAML that deserializes to an empty dict
        var rules = new Dictionary<string, string>
        {
            ["creator"] = "{}",
        };
        var errors = RuleValidator.ValidateRules(rules);
        Assert.True(errors.ContainsKey("creator"));
    }
}
