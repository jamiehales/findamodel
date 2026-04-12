using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class TagGenerationServiceTests
{
    [Fact]
    public void ComputeGenerationChecksum_ChangesWhenSchemaChanges()
    {
        var model = CreateModel();
        var config = CreateConfig();

        var checksumA = TagGenerationService.ComputeGenerationChecksum(model, config, ["orc", "terrain"]);
        var checksumB = TagGenerationService.ComputeGenerationChecksum(model, config, ["orc", "terrain", "dragon"]);

        Assert.NotEqual(checksumA, checksumB);
    }

    [Fact]
    public void ComputeGenerationChecksum_IsOrderInsensitiveForSchema()
    {
        var model = CreateModel();
        var config = CreateConfig();

        var checksumA = TagGenerationService.ComputeGenerationChecksum(model, config, ["terrain", "orc"]);
        var checksumB = TagGenerationService.ComputeGenerationChecksum(model, config, ["orc", "terrain"]);

        Assert.Equal(checksumA, checksumB);
    }

    [Fact]
    public void NeedsRegeneration_ReturnsTrue_WhenSchemaChanges()
    {
        var model = CreateModel();
        var config = CreateConfig();

        var oldSchema = new List<string> { "orc", "terrain" };
        model.GeneratedTagsStatus = "success";
        model.GeneratedTagsJson = "[\"orc\"]";
        model.GeneratedTagsChecksum = TagGenerationService.ComputeGenerationChecksum(model, config, oldSchema);

        var changedSchema = new List<string> { "orc", "terrain", "dragon" };

        var shouldRegenerate = TagGenerationService.NeedsRegeneration(model, config, changedSchema);

        Assert.True(shouldRegenerate);
    }

    [Fact]
    public void ComputeDescriptionChecksum_ChangesWhenModelNameChanges()
    {
        var config = CreateConfig();
        var modelA = CreateModel();
        var modelB = CreateModel();
        modelA.CalculatedModelName = "Goblin Scout";
        modelB.CalculatedModelName = "Goblin Chief";

        var checksumA = TagGenerationService.ComputeDescriptionChecksum(modelA, config);
        var checksumB = TagGenerationService.ComputeDescriptionChecksum(modelB, config);

        Assert.NotEqual(checksumA, checksumB);
    }

    [Fact]
    public void NeedsDescriptionRegeneration_ReturnsTrue_WhenChecksumDiffers()
    {
        var model = CreateModel();
        var config = CreateConfig();

        model.GeneratedDescription = "A sneaky goblin scout with a spear.";
        model.GeneratedDescriptionChecksum = "outdated";

        var shouldRegenerate = TagGenerationService.NeedsDescriptionRegeneration(model, config);

        Assert.True(shouldRegenerate);
    }

    private static CachedModel CreateModel()
    {
        return new CachedModel
        {
            Id = Guid.NewGuid(),
            FileName = "goblin.stl",
            Directory = "minis",
            Checksum = "abc123",
            FileType = "stl",
            FileSize = 1,
            FileModifiedAt = DateTime.UtcNow,
            CachedAt = DateTime.UtcNow,
        };
    }

    private static AppConfigDto CreateConfig()
    {
        return new AppConfigDto(
            DefaultRaftHeightMm: 2f,
            Theme: "nord",
            TagGenerationEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "qwen2.5vl:7b",
            TagGenerationTimeoutMs: 60000,
            TagGenerationMaxTags: 12,
            TagGenerationMinConfidence: 0.45f);
    }
}
