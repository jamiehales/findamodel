using findamodel.Data.Entities;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ConfigInheritanceResolverTests
{
    // ── GetParentPath ─────────────────────────────────────────────────────────

    [Fact]
    public void GetParentPath_ReturnsNull_ForRootPath()
    {
        Assert.Null(ConfigInheritanceResolver.GetParentPath(""));
    }

    [Fact]
    public void GetParentPath_ReturnsEmptyString_ForTopLevelDirectory()
    {
        Assert.Equal("", ConfigInheritanceResolver.GetParentPath("Fantasy"));
    }

    [Fact]
    public void GetParentPath_ReturnsParentDir_ForNestedDirectory()
    {
        Assert.Equal("Fantasy", ConfigInheritanceResolver.GetParentPath("Fantasy/Elves"));
    }

    [Fact]
    public void GetParentPath_ReturnsCorrectParent_ForDeepDirectory()
    {
        Assert.Equal("Fantasy/Elves", ConfigInheritanceResolver.GetParentPath("Fantasy/Elves/Warriors"));
    }

    // ── GetParentRecord ───────────────────────────────────────────────────────

    [Fact]
    public void GetParentRecord_ReturnsNull_ForRoot()
    {
        var dict = new Dictionary<string, DirectoryConfig> { [""] = new() };
        Assert.Null(ConfigInheritanceResolver.GetParentRecord("", dict));
    }

    [Fact]
    public void GetParentRecord_ReturnsNull_WhenParentNotInDict()
    {
        var dict = new Dictionary<string, DirectoryConfig>();
        Assert.Null(ConfigInheritanceResolver.GetParentRecord("Fantasy", dict));
    }

    [Fact]
    public void GetParentRecord_ReturnsParent_WhenItExistsInDict()
    {
        var parent = new DirectoryConfig { DirectoryPath = "" };
        var dict = new Dictionary<string, DirectoryConfig> { [""] = parent };
        var result = ConfigInheritanceResolver.GetParentRecord("Fantasy", dict);
        Assert.Same(parent, result);
    }

    [Fact]
    public void GetParentRecord_ReturnsCorrectParent_ForNestedDir()
    {
        var root = new DirectoryConfig { DirectoryPath = "" };
        var fantasy = new DirectoryConfig { DirectoryPath = "Fantasy" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = fantasy,
        };
        var result = ConfigInheritanceResolver.GetParentRecord("Fantasy/Elves", dict);
        Assert.Same(fantasy, result);
    }

    // ── ExpandToAllAncestors ──────────────────────────────────────────────────

    [Fact]
    public void ExpandToAllAncestors_AlwaysIncludesRoot()
    {
        var result = ConfigInheritanceResolver.ExpandToAllAncestors([]);
        Assert.Contains("", result);
    }

    [Fact]
    public void ExpandToAllAncestors_WithEmptyEnumerable_ReturnsOnlyRoot()
    {
        var result = ConfigInheritanceResolver.ExpandToAllAncestors([]);
        Assert.Single(result);
        Assert.Contains("", result);
    }

    [Fact]
    public void ExpandToAllAncestors_ExpandsToAllIntermediatePaths()
    {
        var result = ConfigInheritanceResolver.ExpandToAllAncestors(["Fantasy/Elves/Warriors"]);
        Assert.Contains("", result);
        Assert.Contains("Fantasy", result);
        Assert.Contains("Fantasy/Elves", result);
        Assert.Contains("Fantasy/Elves/Warriors", result);
        Assert.Equal(4, result.Count);
    }

    [Fact]
    public void ExpandToAllAncestors_MergesDuplicateAncestors()
    {
        var result = ConfigInheritanceResolver.ExpandToAllAncestors(["Fantasy/A", "Fantasy/B"]);
        // "" + "Fantasy" + "Fantasy/A" + "Fantasy/B" = 4
        Assert.Equal(4, result.Count);
        Assert.Contains("Fantasy", result);
    }

    [Fact]
    public void ExpandToAllAncestors_HandlesTopLevelDirectoryAlone()
    {
        var result = ConfigInheritanceResolver.ExpandToAllAncestors(["Fantasy"]);
        Assert.Equal(2, result.Count);
        Assert.Contains("", result);
        Assert.Contains("Fantasy", result);
    }

    // ── ExpandToDescendants ───────────────────────────────────────────────────

    [Fact]
    public void ExpandToDescendants_RootChanged_IncludesAllKnownDirs()
    {
        var changedRoots = new HashSet<string> { "" };
        var all = new[] { "", "A", "A/B", "B" };
        var result = ConfigInheritanceResolver.ExpandToDescendants(changedRoots, all);
        Assert.Contains("", result);
        Assert.Contains("A", result);
        Assert.Contains("A/B", result);
        Assert.Contains("B", result);
    }

    [Fact]
    public void ExpandToDescendants_SpecificRoot_IncludesOnlyItsDescendants()
    {
        var changedRoots = new HashSet<string> { "Fantasy" };
        var all = new[] { "", "Fantasy", "Fantasy/Elves", "Fantasy/Dwarves", "SciFi" };
        var result = ConfigInheritanceResolver.ExpandToDescendants(changedRoots, all);
        Assert.Contains("Fantasy", result);
        Assert.Contains("Fantasy/Elves", result);
        Assert.Contains("Fantasy/Dwarves", result);
        Assert.DoesNotContain("", result);
        Assert.DoesNotContain("SciFi", result);
    }

    [Fact]
    public void ExpandToDescendants_DoesNotIncludeUnrelated()
    {
        var changedRoots = new HashSet<string> { "A" };
        var all = new[] { "", "A", "AB", "B" }; // "AB" should not match prefix "A/"
        var result = ConfigInheritanceResolver.ExpandToDescendants(changedRoots, all);
        Assert.Contains("A", result);
        Assert.DoesNotContain("AB", result);
        Assert.DoesNotContain("B", result);
    }

    // ── ApplyRawFields ────────────────────────────────────────────────────────

    [Fact]
    public void ApplyRawFields_WithNullFields_ClearsAllRawProperties()
    {
        var record = new DirectoryConfig
        {
            RawCreator = "someone",
            RawCollection = "col",
            RawCategory = "cat",
        };
        ConfigInheritanceResolver.ApplyRawFields(record, null);
        Assert.Null(record.RawCreator);
        Assert.Null(record.RawCollection);
        Assert.Null(record.RawCategory);
        Assert.Null(record.RawType);
        Assert.Null(record.RawMaterial);
        Assert.Null(record.RawSupported);
        Assert.Null(record.RawRaftHeightMm);
        Assert.Null(record.RawModelName);
        Assert.Null(record.RawRulesYaml);
    }

    [Fact]
    public void ApplyRawFields_WithValues_SetsAllRawProperties()
    {
        var record = new DirectoryConfig();
        var fields = new RawConfigFields(
            Creator: "Alice",
            Collection: "Fantasy",
            Subcollection: "Elves",
            Tags: ["32mm", "small"],
            Category: "miniature",
            Type: "tabletop",
            Material: "resin",
            Supported: true,
            RaftHeightMm: 1.5f,
            ModelName: "Elf Warrior",
            RulesYaml: "creator:\n  rule: filename");
        ConfigInheritanceResolver.ApplyRawFields(record, fields);
        Assert.Equal("Alice", record.RawCreator);
        Assert.Equal("Fantasy", record.RawCollection);
        Assert.Equal("Elves", record.RawSubcollection);
        Assert.Equal("[\"32mm\",\"small\"]", record.RawTagsJson);
        Assert.Equal("miniature", record.RawCategory);
        Assert.Equal("tabletop", record.RawType);
        Assert.Equal("resin", record.RawMaterial);
        Assert.True(record.RawSupported);
        Assert.Equal(1.5f, record.RawRaftHeightMm);
        Assert.Equal("Elf Warrior", record.RawModelName);
        Assert.Equal("creator:\n  rule: filename", record.RawRulesYaml);
    }

    [Fact]
    public void ResolveFields_AdditiveTags_MergesAllAncestorTagLists()
    {
        var root = new DirectoryConfig { DirectoryPath = "", RawTagsJson = "[\"32mm\",\"small\"]" };
        var mid = new DirectoryConfig { DirectoryPath = "Fantasy", RawTagsJson = "[\"monster\"]" };
        var leaf = new DirectoryConfig { DirectoryPath = "Fantasy/Elites", RawTagsJson = "[\"metal\"]" };

        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = mid,
            ["Fantasy/Elites"] = leaf,
        };

        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(mid, dict);
        ConfigInheritanceResolver.ResolveFields(leaf, dict);

        Assert.Equal("[\"32mm\",\"metal\",\"monster\",\"small\"]", leaf.TagsJson);
    }

    // ── ResolveFields ─────────────────────────────────────────────────────────

    [Fact]
    public void ResolveFields_WithNoParent_UsesOwnRawValues()
    {
        var record = new DirectoryConfig
        {
            DirectoryPath = "",
            RawCreator = "Alice",
            RawCollection = "Fantasy",
        };
        var dict = new Dictionary<string, DirectoryConfig> { [""] = record };

        ConfigInheritanceResolver.ResolveFields(record, dict);

        Assert.Equal("Alice", record.Creator);
        Assert.Equal("Fantasy", record.Collection);
    }

    [Fact]
    public void ResolveFields_InheritsFromParent_WhenLocalValueIsNull()
    {
        var root = new DirectoryConfig { DirectoryPath = "", RawCreator = "Alice" };
        var child = new DirectoryConfig { DirectoryPath = "Fantasy" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = child,
        };
        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(child, dict);

        Assert.Equal("Alice", child.Creator);
    }

    [Fact]
    public void ResolveFields_OwnRawValueWins_OverParent()
    {
        var root = new DirectoryConfig { DirectoryPath = "", RawCreator = "Bob" };
        var child = new DirectoryConfig { DirectoryPath = "Fantasy", RawCreator = "Alice" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = child,
        };
        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(child, dict);

        Assert.Equal("Alice", child.Creator);
    }

    [Fact]
    public void ResolveFields_InheritsRaftHeight_WhenNotSetLocally()
    {
        var root = new DirectoryConfig { DirectoryPath = "", RawRaftHeightMm = 3.0f };
        var child = new DirectoryConfig { DirectoryPath = "Fantasy" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = child,
        };
        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(child, dict);

        Assert.Equal(3.0f, child.RaftHeightMm);
    }

    [Fact]
    public void ResolveFields_OwnRaftHeightWins_OverParent()
    {
        var root = new DirectoryConfig { DirectoryPath = "", RawRaftHeightMm = 3.0f };
        var child = new DirectoryConfig { DirectoryPath = "Fantasy", RawRaftHeightMm = 1.0f };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = child,
        };
        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(child, dict);

        Assert.Equal(1.0f, child.RaftHeightMm);
    }

    [Fact]
    public void ResolveFields_NoRulesYaml_SetsResolvedRulesYamlToNull()
    {
        var record = new DirectoryConfig { DirectoryPath = "" };
        var dict = new Dictionary<string, DirectoryConfig> { [""] = record };

        ConfigInheritanceResolver.ResolveFields(record, dict);

        Assert.Null(record.ResolvedRulesYaml);
    }

    [Fact]
    public void ResolveFields_WithLocalRules_SetsResolvedRulesYaml()
    {
        var record = new DirectoryConfig
        {
            DirectoryPath = "",
            RawRulesYaml = "creator:\n  rule: filename\n",
        };
        var dict = new Dictionary<string, DirectoryConfig> { [""] = record };

        ConfigInheritanceResolver.ResolveFields(record, dict);

        Assert.NotNull(record.ResolvedRulesYaml);
        Assert.Contains("creator", record.ResolvedRulesYaml);
    }

    [Fact]
    public void ResolveFields_InheritsParentRules_WhenChildHasNoRulesForField()
    {
        var root = new DirectoryConfig
        {
            DirectoryPath = "",
            RawRulesYaml = "creator:\n  rule: filename\n",
        };
        var child = new DirectoryConfig { DirectoryPath = "Fantasy" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = child,
        };
        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(child, dict);

        Assert.NotNull(child.ResolvedRulesYaml);
        Assert.Contains("creator", child.ResolvedRulesYaml);
    }

    [Fact]
    public void ResolveFields_OwnRawValueBlocksParentRule_ForSameField()
    {
        var root = new DirectoryConfig
        {
            DirectoryPath = "",
            RawRulesYaml = "creator:\n  rule: filename\n",
        };
        var child = new DirectoryConfig
        {
            DirectoryPath = "Fantasy",
            RawCreator = "Alice",
        };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["Fantasy"] = child,
        };
        ConfigInheritanceResolver.ResolveFields(root, dict);
        ConfigInheritanceResolver.ResolveFields(child, dict);

        // Child has a plain Raw value for creator, so the parent rule should NOT be inherited
        Assert.Equal("Alice", child.Creator);
        // ResolvedRulesYaml should not contain "creator" since the plain value claimed the field
        Assert.True(child.ResolvedRulesYaml == null || !child.ResolvedRulesYaml.Contains("creator"));
    }

    // ── ResolveDescendants ────────────────────────────────────────────────────

    [Fact]
    public void ResolveDescendants_UpdatesAllDescendants_WhenRootChanges()
    {
        var root = new DirectoryConfig { DirectoryPath = "", RawCreator = "Alice" };
        var a = new DirectoryConfig { DirectoryPath = "A" };
        var ab = new DirectoryConfig { DirectoryPath = "A/B" };
        var b = new DirectoryConfig { DirectoryPath = "B" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["A"] = a,
            ["A/B"] = ab,
            ["B"] = b,
        };
        // Resolve root first
        ConfigInheritanceResolver.ResolveFields(root, dict);

        // Now resolve all descendants of root (all dirs)
        ConfigInheritanceResolver.ResolveDescendants("", dict);

        Assert.Equal("Alice", a.Creator);
        Assert.Equal("Alice", ab.Creator);
        Assert.Equal("Alice", b.Creator);
    }

    [Fact]
    public void ResolveDescendants_OnlyAffectsDescendants_NotUnrelated()
    {
        var root = new DirectoryConfig { DirectoryPath = "" };
        var a = new DirectoryConfig { DirectoryPath = "A", RawCreator = "Alice" };
        var ab = new DirectoryConfig { DirectoryPath = "A/B" };
        var b = new DirectoryConfig { DirectoryPath = "B", RawCreator = "Bob" };
        var dict = new Dictionary<string, DirectoryConfig>
        {
            [""] = root,
            ["A"] = a,
            ["A/B"] = ab,
            ["B"] = b,
        };
        ConfigInheritanceResolver.ResolveFields(a, dict);
        ConfigInheritanceResolver.ResolveDescendants("A", dict);

        // A/B inherits from A
        Assert.Equal("Alice", ab.Creator);
        // B is not a descendant of A
        Assert.Null(b.Creator);
    }
}
