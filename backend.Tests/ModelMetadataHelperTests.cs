using findamodel.Data.Entities;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ModelMetadataHelperTests
{
    // ── Null dirConfig ────────────────────────────────────────────────────────

    [Fact]
    public void Compute_NullDirConfig_ReturnsEmptyMetadata()
    {
        var result = ModelMetadataHelper.Compute("/models/dragon.stl", null);

        Assert.Null(result.Creator);
        Assert.Null(result.Collection);
        Assert.Null(result.Subcollection);
        Assert.Null(result.Category);
        Assert.Null(result.Type);
        Assert.Null(result.Material);
        Assert.Null(result.Supported);
        Assert.Null(result.ModelName);
    }

    // ── Plain resolved values ─────────────────────────────────────────────────

    [Fact]
    public void Compute_PlainResolvedValues_ReturnedDirectly()
    {
        var dirConfig = new DirectoryConfig
        {
            Creator = "Alice",
            Collection = "Fantasy",
            Subcollection = "Elves",
            Category = "miniature",
            Type = "tabletop",
            Material = "resin",
            Supported = true,
            ModelName = "Elf Warrior",
        };

        var result = ModelMetadataHelper.Compute("/models/elf_warrior.stl", dirConfig);

        Assert.Equal("Alice", result.Creator);
        Assert.Equal("Fantasy", result.Collection);
        Assert.Equal("Elves", result.Subcollection);
        Assert.Equal("miniature", result.Category);
        Assert.Equal("tabletop", result.Type);
        Assert.Equal("resin", result.Material);
        Assert.True(result.Supported);
        Assert.Equal("Elf Warrior", result.ModelName);
    }

    // ── Rules ─────────────────────────────────────────────────────────────────

    [Fact]
    public void Compute_FilenameRule_OverridesPlainValue()
    {
        // ResolvedRulesYaml takes precedence over the plain resolved field
        var dirConfig = new DirectoryConfig
        {
            ModelName = "Static Name",
            ResolvedRulesYaml = "model_name:\n  rule: filename\n",
        };

        var result = ModelMetadataHelper.Compute("/models/fantasy/dragon.stl", dirConfig);

        // filename rule title-cases the base name (single word, no underscores)
        Assert.Equal("Dragon", result.ModelName);
    }

    [Fact]
    public void Compute_RegexRule_ExtractsGroup()
    {
        var dirConfig = new DirectoryConfig
        {
            ResolvedRulesYaml = "creator:\n  rule: regex\n  source: folder\n  expression: \"^([^/]+)\"\n",
        };

        var result = ModelMetadataHelper.Compute("Fantasy/Elves/warrior.stl", dirConfig);

        Assert.Equal("Fantasy", result.Creator);
    }

    [Fact]
    public void Compute_BoolRule_ParsesStringToBool()
    {
        var dirConfig = new DirectoryConfig
        {
            ResolvedRulesYaml = "supported:\n  rule: regex\n  expression: \"supported\"\n",
        };

        var trueResult = ModelMetadataHelper.Compute("/models/supported_model.stl", dirConfig);
        var falseResult = ModelMetadataHelper.Compute("/models/plain_model.stl", dirConfig);

        Assert.True(trueResult.Supported);
        Assert.False(falseResult.Supported);
    }

    [Fact]
    public void Compute_BoolRule_NoExpression_ReturnsNullForSupportedField()
    {
        // A regex rule with no expression returns null from the parser → Supported stays null
        var dirConfig = new DirectoryConfig
        {
            ResolvedRulesYaml = "supported:\n  rule: regex\n  source: filename\n",
        };

        var result = ModelMetadataHelper.Compute("/models/yes_model.stl", dirConfig);

        // No expression provided → RegexRuleParser returns null → EvaluateBool returns null
        Assert.Null(result.Supported);
    }

    [Fact]
    public void Compute_UnknownRuleInYaml_ReturnsNullForThatField()
    {
        var dirConfig = new DirectoryConfig
        {
            ResolvedRulesYaml = "creator:\n  rule: nonexistent\n",
        };

        var result = ModelMetadataHelper.Compute("/models/model.stl", dirConfig);

        Assert.Null(result.Creator);
    }

    // ── Mixed values and rules ────────────────────────────────────────────────

    [Fact]
    public void Compute_MixesRulesAndPlainValues_AcrossDifferentFields()
    {
        var dirConfig = new DirectoryConfig
        {
            Creator = "Alice",   // plain value
            Collection = "Fantasy",
            ResolvedRulesYaml = "model_name:\n  rule: filename\n",  // rule applies only to model_name
        };

        var result = ModelMetadataHelper.Compute("/models/dragon.stl", dirConfig);

        Assert.Equal("Alice", result.Creator);
        Assert.Equal("Fantasy", result.Collection);
        Assert.Equal("Dragon", result.ModelName);
    }

    // ── model_metadata overrides ──────────────────────────────────────────────

    [Fact]
    public void Compute_ModelMetadataName_OverridesResolvedModelName()
    {
        var dirConfig = new DirectoryConfig
        {
            ModelName = "Generic Name",
            RawModelMetadataJson = """{"dragon.stl":{"Name":"Fire Dragon","PartName":null}}"""
        };

        var result = ModelMetadataHelper.Compute("/models/dragon.stl", dirConfig);

        Assert.Equal("Fire Dragon", result.ModelName);
    }

    [Fact]
    public void Compute_ModelMetadataPartName_SetFromConfig()
    {
        var dirConfig = new DirectoryConfig
        {
            RawModelMetadataJson = """{"bust_torso.stl":{"Name":"Knight Bust","PartName":"Torso"}}"""
        };

        var result = ModelMetadataHelper.Compute("/models/bust_torso.stl", dirConfig);

        Assert.Equal("Knight Bust", result.ModelName);
        Assert.Equal("Torso", result.PartName);
    }

    [Fact]
    public void Compute_ModelMetadata_FilenameLookupIsCaseInsensitive()
    {
        var dirConfig = new DirectoryConfig
        {
            RawModelMetadataJson = """{"Dragon.STL":{"Name":"Fire Dragon","PartName":null}}"""
        };

        var result = ModelMetadataHelper.Compute("/models/dragon.stl", dirConfig);

        Assert.Equal("Fire Dragon", result.ModelName);
    }

    [Fact]
    public void Compute_ModelMetadata_UnmatchedFilename_DoesNotOverride()
    {
        var dirConfig = new DirectoryConfig
        {
            ModelName = "Generic Name",
            RawModelMetadataJson = """{"other.stl":{"Name":"Other","PartName":null}}"""
        };

        var result = ModelMetadataHelper.Compute("/models/dragon.stl", dirConfig);

        Assert.Equal("Generic Name", result.ModelName);
        Assert.Null(result.PartName);
    }

    [Fact]
    public void Compute_AdditiveTags_MergesResolvedFolderAndModelTags()
    {
        var dirConfig = new DirectoryConfig
        {
            TagsJson = "[\"32mm\",\"small\",\"monster\"]",
            RawModelMetadataJson = """{"dragon.stl":{"Tags":["metal"]}}"""
        };

        var result = ModelMetadataHelper.Compute("/models/dragon.stl", dirConfig);

        Assert.Contains("32mm", result.Tags);
        Assert.Contains("small", result.Tags);
        Assert.Contains("monster", result.Tags);
        Assert.Contains("metal", result.Tags);
        Assert.Equal(4, result.Tags.Count);
    }

    [Fact]
    public void Compute_NullModelMetadataJson_PartNameIsNull()
    {
        var dirConfig = new DirectoryConfig
        {
            ModelName = "Generic Name",
            RawModelMetadataJson = null
        };

        var result = ModelMetadataHelper.Compute("/models/dragon.stl", dirConfig);

        Assert.Equal("Generic Name", result.ModelName);
        Assert.Null(result.PartName);
    }

    // ── GetModelMetadataEntry ─────────────────────────────────────────────────

    [Fact]
    public void GetModelMetadataEntry_ReturnsEntry_ForMatchingFilename()
    {
        var dirConfig = new DirectoryConfig
        {
            RawModelMetadataJson = """{"dragon.stl":{"Name":"Fire Dragon","PartName":"Body"}}"""
        };

        var entry = ModelMetadataHelper.GetModelMetadataEntry(dirConfig, "dragon.stl");

        Assert.NotNull(entry);
        Assert.Equal("Fire Dragon", entry.Name);
        Assert.Equal("Body", entry.PartName);
    }

    [Fact]
    public void GetModelMetadataEntry_ReturnsNull_ForNullJson()
    {
        var dirConfig = new DirectoryConfig { RawModelMetadataJson = null };

        var entry = ModelMetadataHelper.GetModelMetadataEntry(dirConfig, "dragon.stl");

        Assert.Null(entry);
    }

    [Fact]
    public void GetModelMetadataEntry_ReturnsNull_ForMissingFilename()
    {
        var dirConfig = new DirectoryConfig
        {
            RawModelMetadataJson = """{"other.stl":{"Name":"Other","PartName":null}}"""
        };

        var entry = ModelMetadataHelper.GetModelMetadataEntry(dirConfig, "dragon.stl");

        Assert.Null(entry);
    }
}
