using findamodel.Data.Entities;
using findamodel.Services;
using findamodel.Services.Rules;
using Xunit;

namespace findamodel.Tests;

public class MetadataFieldRegistryTests
{
    // ── TryGet ────────────────────────────────────────────────────────────────

    [Fact]
    public void TryGet_ReturnsFalse_ForUnknownKey()
    {
        Assert.False(MetadataFieldRegistry.TryGet("nonexistent", out _));
    }

    [Theory]
    [InlineData("creator")]
    [InlineData("collection")]
    [InlineData("subcollection")]
    [InlineData("tags")]
    [InlineData("category")]
    [InlineData("type")]
    [InlineData("material")]
    [InlineData("supported")]
    [InlineData("model_name")]
    [InlineData("part_name")]
    public void TryGet_ReturnsTrue_ForAllKnownKeys(string key)
    {
        Assert.True(MetadataFieldRegistry.TryGet(key, out _));
    }

    [Theory]
    [InlineData("CREATOR")]
    [InlineData("Creator")]
    [InlineData("CATEGORY")]
    [InlineData("Model_Name")]
    public void TryGet_IsCaseInsensitive(string key)
    {
        Assert.True(MetadataFieldRegistry.TryGet(key, out _));
    }

    [Fact]
    public void TryGet_ReturnsDefinition_WithCorrectKey()
    {
        MetadataFieldRegistry.TryGet("creator", out var def);
        Assert.Equal("creator", def.Key);
    }

    // ── Keys ──────────────────────────────────────────────────────────────────

    [Fact]
    public void Keys_ContainsAllTenExpectedFields()
    {
        var keys = MetadataFieldRegistry.Keys.ToHashSet(StringComparer.OrdinalIgnoreCase);
        Assert.Contains("creator", keys);
        Assert.Contains("collection", keys);
        Assert.Contains("subcollection", keys);
        Assert.Contains("tags", keys);
        Assert.Contains("category", keys);
        Assert.Contains("type", keys);
        Assert.Contains("material", keys);
        Assert.Contains("supported", keys);
        Assert.Contains("model_name", keys);
        Assert.Contains("part_name", keys);
        Assert.Equal(10, keys.Count);
    }

    // ── Definitions ───────────────────────────────────────────────────────────

    [Fact]
    public void Definitions_ContainsTenEntries()
    {
        Assert.Equal(10, MetadataFieldRegistry.Definitions.Length);
    }

    // ── GetRuleFieldType ──────────────────────────────────────────────────────

    [Theory]
    [InlineData("creator", RuleFieldType.String)]
    [InlineData("collection", RuleFieldType.String)]
    [InlineData("subcollection", RuleFieldType.String)]
    [InlineData("tags", RuleFieldType.String)]
    [InlineData("model_name", RuleFieldType.String)]
    [InlineData("part_name", RuleFieldType.String)]
    public void GetRuleFieldType_ReturnsString_ForStringFields(string key, RuleFieldType expected)
    {
        Assert.Equal(expected, MetadataFieldRegistry.GetRuleFieldType(key));
    }

    [Theory]
    [InlineData("category", RuleFieldType.Enum)]
    [InlineData("type", RuleFieldType.Enum)]
    [InlineData("material", RuleFieldType.Enum)]
    public void GetRuleFieldType_ReturnsEnum_ForEnumFields(string key, RuleFieldType expected)
    {
        Assert.Equal(expected, MetadataFieldRegistry.GetRuleFieldType(key));
    }

    [Fact]
    public void GetRuleFieldType_ReturnsBool_ForSupportedField()
    {
        Assert.Equal(RuleFieldType.Bool, MetadataFieldRegistry.GetRuleFieldType("supported"));
    }

    [Fact]
    public void GetRuleFieldType_ReturnsString_ForUnknownKey()
    {
        // Fallback to String for unknown fields
        Assert.Equal(RuleFieldType.String, MetadataFieldRegistry.GetRuleFieldType("unknownfield"));
    }

    // ── Accessor round-trips ──────────────────────────────────────────────────

    [Fact]
    public void GetRawValue_And_SetResolvedValue_RoundTrip_ForCreator()
    {
        MetadataFieldRegistry.TryGet("creator", out var def);
        var record = new DirectoryConfig { RawCreator = "Alice" };

        var raw = def.GetRawValue(record);
        Assert.Equal("Alice", raw);

        def.SetResolvedValue(record, "Bob");
        Assert.Equal("Bob", record.Creator);
    }

    [Fact]
    public void GetResolvedValue_ReturnsNull_ForUnsetField()
    {
        MetadataFieldRegistry.TryGet("collection", out var def);
        var record = new DirectoryConfig();
        Assert.Null(def.GetResolvedValue(record));
    }

    [Fact]
    public void SetResolvedValue_ForSupported_SetsNullableBool()
    {
        MetadataFieldRegistry.TryGet("supported", out var def);
        var record = new DirectoryConfig();

        def.SetResolvedValue(record, true);
        Assert.True(record.Supported);

        def.SetResolvedValue(record, null);
        Assert.Null(record.Supported);
    }
}
