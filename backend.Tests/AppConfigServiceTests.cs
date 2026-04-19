using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Xunit;

namespace findamodel.Tests;

public class AppConfigServiceTests
{
    // ── Helpers ───────────────────────────────────────────────────────────────

    private static IDbContextFactory<ModelCacheContext> CreateFactory(string dbName)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;
        return new InMemoryDbContextFactory(options);
    }

    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);
        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
            => Task.FromResult(CreateDbContext());
    }

    private static IConfiguration CreateConfiguration(Dictionary<string, string?>? values = null)
    {
        return new ConfigurationBuilder()
            .AddInMemoryCollection(values)
            .Build();
    }

    // ── GetDefaultRaftHeightMmAsync ───────────────────────────────────────────

    [Fact]
    public async Task GetDefaultRaftHeightMmAsync_ReturnsDefault_WhenDbEmpty()
    {
        var sut = new AppConfigService(CreateFactory(nameof(GetDefaultRaftHeightMmAsync_ReturnsDefault_WhenDbEmpty)), CreateConfiguration());
        var result = await sut.GetDefaultRaftHeightMmAsync();
        Assert.Equal(AppConfigService.DatabaseDefaultRaftHeightMm, result);
    }

    [Fact]
    public async Task GetDefaultRaftHeightMmAsync_ReturnsStoredValue()
    {
        var factory = CreateFactory(nameof(GetDefaultRaftHeightMmAsync_ReturnsStoredValue));
        await using (var seed = factory.CreateDbContext())
        {
            seed.AppConfigs.Add(new AppConfig { Id = 1, DefaultRaftHeightMm = 3.5f });
            await seed.SaveChangesAsync();
        }
        var sut = new AppConfigService(factory, CreateConfiguration());
        Assert.Equal(3.5f, await sut.GetDefaultRaftHeightMmAsync());
    }

    // ── GetAsync ──────────────────────────────────────────────────────────────

    [Fact]
    public async Task GetAsync_CreatesDefaultRecord_WhenDbIsEmpty()
    {
        var sut = new AppConfigService(CreateFactory(nameof(GetAsync_CreatesDefaultRecord_WhenDbIsEmpty)), CreateConfiguration());
        var dto = await sut.GetAsync();
        Assert.Equal(AppConfigService.DatabaseDefaultRaftHeightMm, dto.DefaultRaftHeightMm);
        Assert.True(dto.GeneratePreviewsEnabled);
        Assert.Equal(ModelPreviewService.CurrentPreviewGenerationVersion, dto.MinimumPreviewGenerationVersion);
        Assert.False(dto.TagGenerationEnabled);
        Assert.False(dto.AiDescriptionEnabled);
        Assert.Equal("internal", dto.TagGenerationProvider);
        Assert.Equal(AppConfigService.DefaultTagGenerationModel, dto.TagGenerationModel);
        Assert.Equal(2f, dto.AutoSupportBedMarginMm);
        Assert.Equal(0.8f, dto.AutoSupportMinVoxelSizeMm);
        Assert.Equal(2.0f, dto.AutoSupportMaxVoxelSizeMm);
        Assert.Equal(0.75f, dto.AutoSupportMinLayerHeightMm);
        Assert.Equal(1.5f, dto.AutoSupportMaxLayerHeightMm);
        Assert.Equal(2.5f, dto.AutoSupportMergeDistanceMm);
        Assert.Equal(4f, dto.AutoSupportMinIslandAreaMm2);
        Assert.Equal(10f, dto.AutoSupportMaxSupportDistanceMm);
        Assert.Equal(3f, dto.AutoSupportPullForceThreshold);
        Assert.Equal(1.2f, dto.AutoSupportSphereRadiusMm);
        Assert.Equal(6, dto.AutoSupportMaxSupportsPerIsland);
        Assert.Equal(20f, dto.AutoSupportCrushForceThreshold);
        Assert.Equal(40f, dto.AutoSupportMaxAngularForce);
    }

    [Fact]
    public async Task GetAsync_IsIdempotent_DoesNotCreateDuplicates()
    {
        var factory = CreateFactory(nameof(GetAsync_IsIdempotent_DoesNotCreateDuplicates));
        var sut = new AppConfigService(factory, CreateConfiguration());
        await sut.GetAsync();
        await sut.GetAsync();

        await using var db = factory.CreateDbContext();
        Assert.Equal(1, await db.AppConfigs.CountAsync());
    }

    [Fact]
    public async Task UpdateAsync_DoesNotPersistPromptOverrides_WhenSavingConfiguredDefaults()
    {
        const string configuredTagPrompt = "Configured tag prompt {{maxTags}} {{allowedTags}}";
        const string configuredDescriptionPrompt = "Configured description prompt {{modelName}} {{fullPath}}";
        var factory = CreateFactory(nameof(UpdateAsync_DoesNotPersistPromptOverrides_WhenSavingConfiguredDefaults));
        var sut = new AppConfigService(factory, CreateConfiguration(new Dictionary<string, string?>
        {
            ["AppConfig:TagGenerationPromptTemplate"] = configuredTagPrompt,
            ["AppConfig:DescriptionGenerationPromptTemplate"] = configuredDescriptionPrompt,
        }));

        var updated = await sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: ModelPreviewService.CurrentPreviewGenerationVersion,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "",
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: configuredTagPrompt,
            DescriptionGenerationPromptTemplate: configuredDescriptionPrompt));

        Assert.Equal(configuredTagPrompt, updated.TagGenerationPromptTemplate);
        Assert.Equal(configuredDescriptionPrompt, updated.DescriptionGenerationPromptTemplate);
        Assert.Equal(string.Empty, updated.TagGenerationPromptTemplateOverride);
        Assert.Equal(string.Empty, updated.DescriptionGenerationPromptTemplateOverride);

        await using var db = factory.CreateDbContext();
        var stored = await db.AppConfigs.SingleAsync();
        Assert.Equal(string.Empty, stored.TagGenerationPromptTemplate);
        Assert.Equal(string.Empty, stored.DescriptionGenerationPromptTemplate);
    }

    [Fact]
    public async Task UpdateAsync_AllowsEmptyPromptOverrides_AndReturnsEffectiveDefaults()
    {
        const string configuredTagPrompt = "Configured tag prompt {{maxTags}} {{allowedTags}}";
        const string configuredDescriptionPrompt = "Configured description prompt {{modelName}} {{fullPath}}";
        var factory = CreateFactory(nameof(UpdateAsync_AllowsEmptyPromptOverrides_AndReturnsEffectiveDefaults));
        var sut = new AppConfigService(factory, CreateConfiguration(new Dictionary<string, string?>
        {
            ["AppConfig:TagGenerationPromptTemplate"] = configuredTagPrompt,
            ["AppConfig:DescriptionGenerationPromptTemplate"] = configuredDescriptionPrompt,
        }));

        var updated = await sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: ModelPreviewService.CurrentPreviewGenerationVersion,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "",
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: string.Empty,
            DescriptionGenerationPromptTemplate: string.Empty));

        Assert.Equal(configuredTagPrompt, updated.TagGenerationPromptTemplate);
        Assert.Equal(configuredDescriptionPrompt, updated.DescriptionGenerationPromptTemplate);
        Assert.Equal(string.Empty, updated.TagGenerationPromptTemplateOverride);
        Assert.Equal(string.Empty, updated.DescriptionGenerationPromptTemplateOverride);
    }

    // ── UpdateDefaultRaftHeightAsync ──────────────────────────────────────────

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_PersistsNewValue()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_PersistsNewValue)), CreateConfiguration());
        var dto = await sut.UpdateDefaultRaftHeightAsync(4f);
        Assert.Equal(4f, dto.DefaultRaftHeightMm);

        // Verify the updated value is read back on subsequent call
        Assert.Equal(4f, await sut.GetDefaultRaftHeightMmAsync());
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_AcceptsZero()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_AcceptsZero)), CreateConfiguration());
        var dto = await sut.UpdateDefaultRaftHeightAsync(0f);
        Assert.Equal(0f, dto.DefaultRaftHeightMm);
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForNegativeValue()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForNegativeValue)), CreateConfiguration());
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(-1f));
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForNaN()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForNaN)), CreateConfiguration());
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(float.NaN));
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForPositiveInfinity()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForPositiveInfinity)), CreateConfiguration());
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(float.PositiveInfinity));
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForNegativeInfinity()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForNegativeInfinity)), CreateConfiguration());
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(float.NegativeInfinity));
    }

    [Fact]
    public async Task UpdateAsync_PersistsTagGenerationSettings()
    {
        var factory = CreateFactory(nameof(UpdateAsync_PersistsTagGenerationSettings));
        var sut = new AppConfigService(factory, CreateConfiguration());

        var updated = await sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: 5,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "ollama",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: AppConfigService.DefaultTagGenerationModel,
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: "Tag prompt {{maxTags}} {{allowedTags}}",
            DescriptionGenerationPromptTemplate: "Describe {{modelName}}"));

        Assert.True(updated.TagGenerationEnabled);
        Assert.True(updated.AiDescriptionEnabled);
        Assert.Equal(5, updated.MinimumPreviewGenerationVersion);
        Assert.Equal("ollama", updated.TagGenerationProvider);
        Assert.Equal(45000, updated.TagGenerationTimeoutMs);
        Assert.Equal(10, updated.TagGenerationMaxTags);
        Assert.Equal(0.5f, updated.TagGenerationMinConfidence);

        await using var db = factory.CreateDbContext();
        var stored = await db.AppConfigs.SingleAsync();
        Assert.Equal(5, stored.MinimumPreviewGenerationVersion);
        Assert.Null(stored.TagGenerationModel);
    }

    [Fact]
    public async Task UpdateAsync_PersistsAutoSupportSettings()
    {
        var factory = CreateFactory(nameof(UpdateAsync_PersistsAutoSupportSettings));
        var sut = new AppConfigService(factory, CreateConfiguration());

        var updated = await sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: 5,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "",
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: "Tag prompt {{maxTags}} {{allowedTags}}",
            DescriptionGenerationPromptTemplate: "Describe {{modelName}}",
            AutoSupportBedMarginMm: 4f,
            AutoSupportMinVoxelSizeMm: 1.1f,
            AutoSupportMaxVoxelSizeMm: 2.4f,
            AutoSupportMinLayerHeightMm: 0.6f,
            AutoSupportMaxLayerHeightMm: 1.8f,
            AutoSupportMergeDistanceMm: 3.5f,
            AutoSupportMinIslandAreaMm2: 12f,
            AutoSupportMaxSupportDistanceMm: 11f,
            AutoSupportPullForceThreshold: 5.5f,
            AutoSupportSphereRadiusMm: 1.6f,
            AutoSupportMaxSupportsPerIsland: 9,
            AutoSupportCrushForceThreshold: 7.5f,
            AutoSupportMaxAngularForce: 18f,
            AutoSupportV2VoxelSizeMm: 0.25f));

        Assert.Equal(4f, updated.AutoSupportBedMarginMm);
        Assert.Equal(1.1f, updated.AutoSupportMinVoxelSizeMm);
        Assert.Equal(2.4f, updated.AutoSupportMaxVoxelSizeMm);
        Assert.Equal(0.6f, updated.AutoSupportMinLayerHeightMm);
        Assert.Equal(1.8f, updated.AutoSupportMaxLayerHeightMm);
        Assert.Equal(3.5f, updated.AutoSupportMergeDistanceMm);
        Assert.Equal(12f, updated.AutoSupportMinIslandAreaMm2);
        Assert.Equal(11f, updated.AutoSupportMaxSupportDistanceMm);
        Assert.Equal(5.5f, updated.AutoSupportPullForceThreshold);
        Assert.Equal(1.6f, updated.AutoSupportSphereRadiusMm);
        Assert.Equal(9, updated.AutoSupportMaxSupportsPerIsland);
        Assert.Equal(7.5f, updated.AutoSupportCrushForceThreshold);
        Assert.Equal(18f, updated.AutoSupportMaxAngularForce);
        Assert.Equal(0.25f, updated.AutoSupportV2VoxelSizeMm);

        await using var db = factory.CreateDbContext();
        var stored = await db.AppConfigs.SingleAsync();
        Assert.Equal(4f, stored.AutoSupportBedMarginMm);
        Assert.Equal(12f, stored.AutoSupportMinIslandAreaMm2);
        Assert.Equal(11f, stored.AutoSupportMaxSupportDistanceMm);
        Assert.Equal(5.5f, stored.AutoSupportPullForceThreshold);
        Assert.Equal(9, stored.AutoSupportMaxSupportsPerIsland);
        Assert.Equal(7.5f, stored.AutoSupportCrushForceThreshold);
        Assert.Equal(18f, stored.AutoSupportMaxAngularForce);
        Assert.Equal(0.25f, stored.AutoSupportV2VoxelSizeMm);
    }

    [Fact]
    public async Task UpdateAsync_AllowsHighAutoSupportResinStrength()
    {
        var factory = CreateFactory(nameof(UpdateAsync_AllowsHighAutoSupportResinStrength));
        var sut = new AppConfigService(factory, CreateConfiguration());

        var updated = await sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: 5,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "",
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: "Tag prompt {{maxTags}} {{allowedTags}}",
            DescriptionGenerationPromptTemplate: "Describe {{modelName}}",
            AutoSupportResinStrength: 25f));

        Assert.Equal(25f, updated.AutoSupportResinStrength);

        await using var db = factory.CreateDbContext();
        var stored = await db.AppConfigs.SingleAsync();
        Assert.Equal(25f, stored.AutoSupportResinStrength);
    }

    [Fact]
    public async Task UpdateAsync_ThrowsForUnknownTagGenerationProvider()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateAsync_ThrowsForUnknownTagGenerationProvider)), CreateConfiguration());

        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: ModelPreviewService.CurrentPreviewGenerationVersion,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "not-real",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: AppConfigService.DefaultTagGenerationModel,
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: "Tag prompt {{maxTags}} {{allowedTags}}",
            DescriptionGenerationPromptTemplate: "Describe {{modelName}}")));
    }

    [Fact]
    public async Task UpdateAsync_ThrowsForNegativeMinimumPreviewVersion()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateAsync_ThrowsForNegativeMinimumPreviewVersion)), CreateConfiguration());

        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: -1,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: AppConfigService.DefaultTagGenerationModel,
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: "Tag prompt {{maxTags}} {{allowedTags}}",
            DescriptionGenerationPromptTemplate: "Describe {{modelName}}")));
    }

    [Fact]
    public async Task UpdateAsync_ThrowsWhenMinimumPreviewVersionExceedsCurrentRendererVersion()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateAsync_ThrowsWhenMinimumPreviewVersionExceedsCurrentRendererVersion)), CreateConfiguration());

        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            GeneratePreviewsEnabled: true,
            MinimumPreviewGenerationVersion: ModelPreviewService.CurrentPreviewGenerationVersion + 1,
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "internal",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: AppConfigService.DefaultTagGenerationModel,
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f,
            TagGenerationPromptTemplate: "Tag prompt {{maxTags}} {{allowedTags}}",
            DescriptionGenerationPromptTemplate: "Describe {{modelName}}")));
    }

    [Fact]
    public async Task GetSetupStatusAsync_RequiresWizard_WhenNoModelsPathConfigured()
    {
        var sut = new AppConfigService(
            CreateFactory(nameof(GetSetupStatusAsync_RequiresWizard_WhenNoModelsPathConfigured)),
            CreateConfiguration());

        var status = await sut.GetSetupStatusAsync();

        Assert.False(status.SetupCompleted);
        Assert.True(status.RequiresWizard);
    }

    [Fact]
    public async Task CompleteInitialSetupAsync_PersistsModelsPath_AndMarksSetupComplete()
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-setup-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);
        try
        {
            var sut = new AppConfigService(
                CreateFactory(nameof(CompleteInitialSetupAsync_PersistsModelsPath_AndMarksSetupComplete)),
                CreateConfiguration());

            var updated = await sut.CompleteInitialSetupAsync(new(
                ModelsDirectoryPath: modelsRoot,
                DefaultRaftHeightMm: 2f,
                Theme: "nord",
                GeneratePreviewsEnabled: true,
                TagGenerationEnabled: true,
                AiDescriptionEnabled: true,
                TagGenerationProvider: "internal",
                TagGenerationEndpoint: "http://localhost:11434",
                TagGenerationModel: AppConfigService.DefaultTagGenerationModel,
                TagGenerationTimeoutMs: 60000,
                TagGenerationMaxTags: 12,
                TagGenerationMinConfidence: 0.45f));

            Assert.True(updated.SetupCompleted);
            Assert.Equal(Path.GetFullPath(modelsRoot), updated.ModelsDirectoryPath);
            Assert.Equal(ModelPreviewService.CurrentPreviewGenerationVersion, updated.MinimumPreviewGenerationVersion);
        }
        finally
        {
            Directory.Delete(modelsRoot, true);
        }
    }
}
