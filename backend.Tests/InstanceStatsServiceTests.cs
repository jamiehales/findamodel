using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.FileProviders;
using Microsoft.Extensions.Hosting;
using Xunit;

namespace findamodel.Tests;

public class InstanceStatsServiceTests
{
    [Fact]
    public async Task GetAsync_ReturnsCountsVersionsAndGpuFlags()
    {
        var factory = CreateFactory(nameof(GetAsync_ReturnsCountsVersionsAndGpuFlags));
        await using (var seed = factory.CreateDbContext())
        {
            seed.Models.AddRange(
                new CachedModel
                {
                    Id = Guid.NewGuid(),
                    Checksum = "a",
                    FileName = "model-a.stl",
                    Directory = "heroes",
                    FileType = "stl",
                    FileSize = 128,
                    FileModifiedAt = DateTime.UtcNow,
                    CachedAt = DateTime.UtcNow,
                    PreviewImagePath = "a.png",
                    GeneratedTagsJson = "[\"hero\"]",
                    GeneratedDescription = "A hero model."
                },
                new CachedModel
                {
                    Id = Guid.NewGuid(),
                    Checksum = "b",
                    FileName = "model-b.obj",
                    Directory = "terrain",
                    FileType = "obj",
                    FileSize = 256,
                    FileModifiedAt = DateTime.UtcNow,
                    CachedAt = DateTime.UtcNow
                });

            seed.DirectoryConfigs.Add(new DirectoryConfig
            {
                Id = Guid.NewGuid(),
                DirectoryPath = "heroes",
                UpdatedAt = DateTime.UtcNow,
            });

            seed.PrintingLists.Add(new PrintingList
            {
                Id = Guid.NewGuid(),
                Name = "Default",
                OwnerId = Guid.NewGuid(),
                CreatedAt = DateTime.UtcNow,
            });

            seed.MetadataDictionaryValues.Add(new MetadataDictionaryValue
            {
                Id = Guid.NewGuid(),
                Field = "tags",
                Value = "hero",
                NormalizedValue = "hero",
                CreatedAt = DateTime.UtcNow,
                UpdatedAt = DateTime.UtcNow,
            });

            await seed.SaveChangesAsync();
        }

        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Preview:UseGpu"] = "true",
                ["LocalLlm:Internal:UseGpu"] = "true",
                ["LocalLlm:Internal:GpuLayerCount"] = "42",
            })
            .Build();

        var sut = new InstanceStatsService(
            factory,
            new FakePreviewRuntimeInfoProvider(gpuEnabled: true, gpuAvailable: false),
            configuration,
            new TestHostEnvironment("Development"));

        var result = await sut.GetAsync();

        Assert.False(string.IsNullOrWhiteSpace(result.ApplicationVersion));
        Assert.Equal("Development", result.Environment);
        Assert.False(string.IsNullOrWhiteSpace(result.FrameworkVersion));
        Assert.False(string.IsNullOrWhiteSpace(result.OperatingSystem));
        Assert.Equal(ModelPreviewService.CurrentPreviewGenerationVersion, result.PreviewGenerationVersion);
        Assert.Equal(HullCalculationService.CurrentHullGenerationVersion, result.HullGenerationVersion);
        Assert.True(result.PreviewGpuEnabled);
        Assert.False(result.PreviewGpuAvailable);
        Assert.True(result.InternalLlmGpuEnabled);
        Assert.Equal(42, result.InternalLlmGpuLayerCount);
        Assert.Equal(2, result.ModelCount);
        Assert.Equal(1, result.ModelsWithPreviews);
        Assert.Equal(1, result.ModelsWithGeneratedTags);
        Assert.Equal(1, result.ModelsWithGeneratedDescriptions);
        Assert.Equal(1, result.DirectoryConfigCount);
        Assert.Equal(1, result.PrintingListCount);
        Assert.Equal(1, result.MetadataDictionaryValueCount);
    }

    [Fact]
    public async Task GetAsync_ZeroesInternalGpuLayers_WhenGpuDisabled()
    {
        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Preview:UseGpu"] = "false",
                ["LocalLlm:Internal:UseGpu"] = "false",
                ["LocalLlm:Internal:GpuLayerCount"] = "35",
            })
            .Build();

        var sut = new InstanceStatsService(
            CreateFactory(nameof(GetAsync_ZeroesInternalGpuLayers_WhenGpuDisabled)),
            new FakePreviewRuntimeInfoProvider(gpuEnabled: false, gpuAvailable: false),
            configuration,
            new TestHostEnvironment("Production"));

        var result = await sut.GetAsync();

        Assert.False(result.PreviewGpuEnabled);
        Assert.False(result.InternalLlmGpuEnabled);
        Assert.Equal(0, result.InternalLlmGpuLayerCount);
        Assert.Equal(0, result.ModelCount);
    }

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

    private sealed class FakePreviewRuntimeInfoProvider(bool gpuEnabled, bool gpuAvailable)
        : IPreviewRuntimeInfoProvider
    {
        public bool GpuEnabled { get; } = gpuEnabled;

        public bool GpuAvailable { get; } = gpuAvailable;
    }

    private sealed class TestHostEnvironment(string environmentName) : IHostEnvironment
    {
        public string EnvironmentName { get; set; } = environmentName;
        public string ApplicationName { get; set; } = nameof(findamodel);
        public string ContentRootPath { get; set; } = AppContext.BaseDirectory;
        public IFileProvider ContentRootFileProvider { get; set; } = new NullFileProvider();
    }
}