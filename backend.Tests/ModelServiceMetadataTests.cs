using System.Security.Cryptography;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ModelServiceMetadataTests
{
    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);
        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default) =>
            Task.FromResult(CreateDbContext());
    }

    private static IDbContextFactory<ModelCacheContext> CreateFactory(string dbName)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;
        return new InMemoryDbContextFactory(options);
    }

    private static IConfiguration CreateConfiguration(string modelsRoot) =>
        new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Models:DirectoryPath"] = modelsRoot,
                ["Preview:UseGpu"] = "false",
            })
            .Build();

    private static ModelService CreateSut(
        IConfiguration configuration,
        IDbContextFactory<ModelCacheContext> dbFactory)
    {
        var loggerFactory = NullLoggerFactory.Instance;
        var configReader = new DirectoryConfigReader(loggerFactory);
        var metadataConfigService = new MetadataConfigService(configuration, loggerFactory, dbFactory, configReader);
        var appConfigService = new AppConfigService(dbFactory);

        // These are not used in the tested code paths.
        ModelLoaderService loaderService = null!;
        ModelPreviewService previewService = null!;
        HullCalculationService hullCalculationService = null!;

        return new ModelService(
            configuration,
            loggerFactory,
            dbFactory,
            loaderService,
            previewService,
            hullCalculationService,
            metadataConfigService,
            appConfigService);
    }

    [Fact]
    public async Task GetModelMetadataAsync_ReturnsEntry_ForMatchingModelFile()
    {
        var dbFactory = CreateFactory(nameof(GetModelMetadataAsync_ReturnsEntry_ForMatchingModelFile));
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);

        try
        {
            var modelId = Guid.NewGuid();
            await using (var db = await dbFactory.CreateDbContextAsync())
            {
                db.Models.Add(new CachedModel
                {
                    Id = modelId,
                    FileName = "dragon.stl",
                    Directory = "",
                    FileType = "stl",
                    Checksum = "abc",
                    FileSize = 1,
                    FileModifiedAt = DateTime.UtcNow,
                    CachedAt = DateTime.UtcNow,
                });

                db.DirectoryConfigs.Add(new DirectoryConfig
                {
                    Id = Guid.NewGuid(),
                    DirectoryPath = "",
                    RawModelMetadataJson = """
                        {
                          "dragon.stl": {
                            "Name": "Fire Dragon",
                            "PartName": "Body",
                            "Creator": "Alice",
                            "Collection": "Fantasy",
                            "Subcollection": "Creatures",
                            "Category": "miniature",
                            "Type": "whole",
                            "Material": "resin",
                            "Supported": true
                          }
                        }
                        """,
                    UpdatedAt = DateTime.UtcNow,
                });

                await db.SaveChangesAsync();
            }

            var sut = CreateSut(CreateConfiguration(modelsRoot), dbFactory);
            var result = await sut.GetModelMetadataAsync(modelId);

            Assert.NotNull(result);
            Assert.Equal("Fire Dragon", result!.Name);
            Assert.Equal("Body", result.PartName);
            Assert.Equal("Alice", result.Creator);
            Assert.Equal("Fantasy", result.Collection);
            Assert.Equal("Creatures", result.Subcollection);
            Assert.Equal("miniature", result.Category);
            Assert.Equal("whole", result.Type);
            Assert.Equal("resin", result.Material);
            Assert.True(result.Supported);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Fact]
    public async Task UpdateModelMetadataAsync_WritesModelMetadataToFindamodelYaml()
    {
        var dbFactory = CreateFactory(nameof(UpdateModelMetadataAsync_WritesModelMetadataToFindamodelYaml));
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);

        var modelFilePath = Path.Combine(modelsRoot, "dragon.stl");
        await File.WriteAllTextAsync(modelFilePath, "solid dragon\nendsolid dragon\n");

        var checksum = await ComputeChecksumAsync(modelFilePath);
        var modelId = Guid.NewGuid();

        try
        {
            await using (var db = await dbFactory.CreateDbContextAsync())
            {
                db.Models.Add(new CachedModel
                {
                    Id = modelId,
                    FileName = "dragon.stl",
                    Directory = "",
                    FileType = "stl",
                    Checksum = checksum,
                    ScanConfigChecksum = ScanConfig.Compute(HullCalculationService.DefaultRaftHeightMm),
                    PreviewGenerationVersion = ModelPreviewService.CurrentPreviewGenerationVersion,
                    FileSize = new FileInfo(modelFilePath).Length,
                    FileModifiedAt = File.GetLastWriteTimeUtc(modelFilePath),
                    CachedAt = DateTime.UtcNow,
                    CalculatedModelName = "Dragon",
                });

                db.DirectoryConfigs.Add(new DirectoryConfig
                {
                    Id = Guid.NewGuid(),
                    DirectoryPath = "",
                    UpdatedAt = DateTime.UtcNow,
                });

                await db.SaveChangesAsync();
            }

            var sut = CreateSut(CreateConfiguration(modelsRoot), dbFactory);
            var dto = await sut.UpdateModelMetadataAsync(modelId, new UpdateModelMetadataRequest(
                Name: "Fire Dragon",
                PartName: "Body",
                Creator: "Alice",
                Collection: "Fantasy",
                Subcollection: "Creatures",
                Category: "miniature",
                Type: "whole",
                Material: "resin",
                Supported: true));

            Assert.NotNull(dto);

            var configPath = Path.Combine(modelsRoot, DirectoryConfigReader.ConfigFileName);
            Assert.True(File.Exists(configPath));

            var yamlText = await File.ReadAllTextAsync(configPath);
            Assert.Contains("model_metadata:", yamlText);
            Assert.Contains("dragon.stl:", yamlText);
            Assert.Contains("name: Fire Dragon", yamlText);
            Assert.Contains("part_name: Body", yamlText);
            Assert.Contains("creator: Alice", yamlText);
            Assert.Contains("collection: Fantasy", yamlText);
            Assert.Contains("subcollection: Creatures", yamlText);
            Assert.Contains("category: miniature", yamlText);
            Assert.Contains("type: whole", yamlText);
            Assert.Contains("material: resin", yamlText);
            Assert.Contains("supported: true", yamlText);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    private static async Task<string> ComputeChecksumAsync(string filePath)
    {
        await using var stream = File.OpenRead(filePath);
        var hash = await SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
