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
            TagGenerationAutoApply: true,
            TagGenerationMaxTags: 12,
            TagGenerationMinConfidence: 0.45f);
    }
}
