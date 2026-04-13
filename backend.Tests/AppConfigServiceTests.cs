using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
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

    // ── GetDefaultRaftHeightMmAsync ───────────────────────────────────────────

    [Fact]
    public async Task GetDefaultRaftHeightMmAsync_ReturnsDefault_WhenDbEmpty()
    {
        var sut = new AppConfigService(CreateFactory(nameof(GetDefaultRaftHeightMmAsync_ReturnsDefault_WhenDbEmpty)));
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
        var sut = new AppConfigService(factory);
        Assert.Equal(3.5f, await sut.GetDefaultRaftHeightMmAsync());
    }

    // ── GetAsync ──────────────────────────────────────────────────────────────

    [Fact]
    public async Task GetAsync_CreatesDefaultRecord_WhenDbIsEmpty()
    {
        var sut = new AppConfigService(CreateFactory(nameof(GetAsync_CreatesDefaultRecord_WhenDbIsEmpty)));
        var dto = await sut.GetAsync();
        Assert.Equal(AppConfigService.DatabaseDefaultRaftHeightMm, dto.DefaultRaftHeightMm);
        Assert.False(dto.TagGenerationEnabled);
        Assert.False(dto.AiDescriptionEnabled);
        Assert.Equal("internal", dto.TagGenerationProvider);
        Assert.Equal("qwen2.5vl:7b", dto.TagGenerationModel);
    }

    [Fact]
    public async Task GetAsync_IsIdempotent_DoesNotCreateDuplicates()
    {
        var factory = CreateFactory(nameof(GetAsync_IsIdempotent_DoesNotCreateDuplicates));
        var sut = new AppConfigService(factory);
        await sut.GetAsync();
        await sut.GetAsync();

        await using var db = factory.CreateDbContext();
        Assert.Equal(1, await db.AppConfigs.CountAsync());
    }

    // ── UpdateDefaultRaftHeightAsync ──────────────────────────────────────────

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_PersistsNewValue()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_PersistsNewValue)));
        var dto = await sut.UpdateDefaultRaftHeightAsync(4f);
        Assert.Equal(4f, dto.DefaultRaftHeightMm);

        // Verify the updated value is read back on subsequent call
        Assert.Equal(4f, await sut.GetDefaultRaftHeightMmAsync());
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_AcceptsZero()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_AcceptsZero)));
        var dto = await sut.UpdateDefaultRaftHeightAsync(0f);
        Assert.Equal(0f, dto.DefaultRaftHeightMm);
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForNegativeValue()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForNegativeValue)));
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(-1f));
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForNaN()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForNaN)));
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(float.NaN));
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForPositiveInfinity()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForPositiveInfinity)));
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(float.PositiveInfinity));
    }

    [Fact]
    public async Task UpdateDefaultRaftHeightAsync_ThrowsForNegativeInfinity()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateDefaultRaftHeightAsync_ThrowsForNegativeInfinity)));
        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateDefaultRaftHeightAsync(float.NegativeInfinity));
    }

    [Fact]
    public async Task UpdateAsync_PersistsTagGenerationSettings()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateAsync_PersistsTagGenerationSettings)));

        var updated = await sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "ollama",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "qwen2.5vl:7b",
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f));

        Assert.True(updated.TagGenerationEnabled);
        Assert.True(updated.AiDescriptionEnabled);
        Assert.Equal("ollama", updated.TagGenerationProvider);
        Assert.Equal(45000, updated.TagGenerationTimeoutMs);
        Assert.Equal(10, updated.TagGenerationMaxTags);
        Assert.Equal(0.5f, updated.TagGenerationMinConfidence);
    }

    [Fact]
    public async Task UpdateAsync_ThrowsForUnknownTagGenerationProvider()
    {
        var sut = new AppConfigService(CreateFactory(nameof(UpdateAsync_ThrowsForUnknownTagGenerationProvider)));

        await Assert.ThrowsAsync<ArgumentException>(() => sut.UpdateAsync(new(
            DefaultRaftHeightMm: 3f,
            Theme: "nord",
            TagGenerationEnabled: true,
            AiDescriptionEnabled: true,
            TagGenerationProvider: "not-real",
            TagGenerationEndpoint: "http://localhost:11434",
            TagGenerationModel: "qwen2.5vl:7b",
            TagGenerationTimeoutMs: 45000,
            TagGenerationMaxTags: 10,
            TagGenerationMinConfidence: 0.5f)));
    }
}
