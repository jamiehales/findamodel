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
    public void ComputeGenerationChecksum_DoesNotChangeWhenModelChecksumChanges()
    {
        var config = CreateConfig();
        var modelA = CreateModel();
        var modelB = CreateModel();
        modelA.Checksum = "checksum-a";
        modelB.Checksum = "checksum-b";

        var checksumA = TagGenerationService.ComputeGenerationChecksum(modelA, config, ["orc", "terrain"]);
        var checksumB = TagGenerationService.ComputeGenerationChecksum(modelB, config, ["orc", "terrain"]);

        Assert.Equal(checksumA, checksumB);
    }

    [Fact]
    public void ComputeGenerationChecksum_ChangesWhenPreviewTimestampChanges()
    {
        var config = CreateConfig();
        var modelA = CreateModel();
        var modelB = CreateModel();
        modelA.PreviewGeneratedAt = new DateTime(2026, 4, 12, 10, 0, 0, DateTimeKind.Utc);
        modelB.PreviewGeneratedAt = new DateTime(2026, 4, 12, 10, 5, 0, DateTimeKind.Utc);

        var checksumA = TagGenerationService.ComputeGenerationChecksum(modelA, config, ["orc", "terrain"]);
        var checksumB = TagGenerationService.ComputeGenerationChecksum(modelB, config, ["orc", "terrain"]);

        Assert.NotEqual(checksumA, checksumB);
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
    public void NeedsRegeneration_ReturnsFalse_WhenPreviewMissing()
    {
        var model = CreateModel();
        var config = CreateConfig();
        model.PreviewImagePath = null;
        model.PreviewGeneratedAt = null;
        model.GeneratedTagsStatus = "success";
        model.GeneratedTagsChecksum = TagGenerationService.ComputeGenerationChecksum(model, config, ["orc", "terrain"]);

        var shouldRegenerate = TagGenerationService.NeedsRegeneration(model, config, ["orc", "terrain"]);

        Assert.False(shouldRegenerate);
    }

    [Fact]
    public void NeedsRegeneration_ReturnsTrue_WhenPreviewNewerThanGeneratedTags()
    {
        var model = CreateModel();
        var config = CreateConfig();
        model.GeneratedTagsStatus = "success";
        model.GeneratedTagsAt = new DateTime(2026, 4, 12, 10, 0, 0, DateTimeKind.Utc);
        model.PreviewGeneratedAt = new DateTime(2026, 4, 12, 10, 5, 0, DateTimeKind.Utc);
        model.GeneratedTagsChecksum = TagGenerationService.ComputeGenerationChecksum(model, config, ["orc", "terrain"]);

        var shouldRegenerate = TagGenerationService.NeedsRegeneration(model, config, ["orc", "terrain"]);

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
    public void ComputeDescriptionChecksum_ChangesWhenFullPathChanges()
    {
        var config = CreateConfig();
        var modelA = CreateModel();
        var modelB = CreateModel();
        modelA.Directory = "minis/goblins";
        modelB.Directory = "minis/orcs";

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

    [Fact]
    public void NeedsDescriptionRegeneration_ReturnsTrue_WhenPreviewNewerThanDescription()
    {
        var model = CreateModel();
        var config = CreateConfig();
        model.GeneratedDescription = "A sneaky goblin scout with a spear.";
        model.GeneratedDescriptionAt = new DateTime(2026, 4, 12, 10, 0, 0, DateTimeKind.Utc);
        model.PreviewGeneratedAt = new DateTime(2026, 4, 12, 10, 5, 0, DateTimeKind.Utc);
        model.GeneratedDescriptionChecksum = TagGenerationService.ComputeDescriptionChecksum(model, config);

        var shouldRegenerate = TagGenerationService.NeedsDescriptionRegeneration(model, config);

        Assert.True(shouldRegenerate);
    }

    private static CachedModel CreateModel()
    {
        var now = new DateTime(2026, 4, 12, 10, 0, 0, DateTimeKind.Utc);
        return new CachedModel
        {
            Id = Guid.NewGuid(),
            FileName = "goblin.stl",
            Directory = "minis",
            Checksum = "abc123",
            FileType = "stl",
            FileSize = 1,
            FileModifiedAt = now,
            CachedAt = now,
            PreviewImagePath = "abc123.png",
            PreviewGeneratedAt = now,
        };
    }

    private static AppConfigDto CreateConfig()
    {
        return new AppConfigDto(
            DefaultRaftHeightMm: 2f,
            Theme: "nord",
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "qwen2.5vl:7b",
            TagGenerationTimeoutMs: 60000,
            TagGenerationMaxTags: 12,
            TagGenerationMinConfidence: 0.45f,
            TagGenerationPromptTemplate: "Return at most {{maxTags}} tags from {{allowedTags}}.",
            DescriptionGenerationPromptTemplate: "Describe '{{modelName}}' at '{{fullPath}}' in two sentences.",
            TagGenerationPromptTemplateDefault: "Return at most {{maxTags}} tags from {{allowedTags}}.",
            DescriptionGenerationPromptTemplateDefault: "Describe '{{modelName}}' at '{{fullPath}}' in two sentences.",
            TagGenerationPromptTemplateOverride: "Return at most {{maxTags}} tags from {{allowedTags}}.",
            DescriptionGenerationPromptTemplateOverride: "Describe '{{modelName}}' at '{{fullPath}}' in two sentences.",
            SetupCompleted: true,
            ModelsDirectoryPath: "C:/models");
    }
}
