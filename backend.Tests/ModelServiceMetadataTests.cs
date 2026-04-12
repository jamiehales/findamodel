using System.Security.Cryptography;
using System.Reflection;
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

    private static IConfiguration CreateConfiguration(string modelsRoot, int? fileScanThreads = null)
    {
        var values = new Dictionary<string, string?>
        {
            ["Models:DirectoryPath"] = modelsRoot,
            ["Preview:UseGpu"] = "false",
        };

        if (fileScanThreads.HasValue)
            values["Indexing:FileScanThreads"] = fileScanThreads.Value.ToString();

        return new ConfigurationBuilder()
            .AddInMemoryCollection(values)
            .Build();
    }

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
            Assert.Equal("Fire Dragon", result!.LocalValues.Name);
            Assert.Equal("Body", result.LocalValues.PartName);
            Assert.Equal("Alice", result.LocalValues.Creator);
            Assert.Equal("Fantasy", result.LocalValues.Collection);
            Assert.Equal("Creatures", result.LocalValues.Subcollection);
            Assert.Equal("miniature", result.LocalValues.Category);
            Assert.Equal("whole", result.LocalValues.Type);
            Assert.Equal("resin", result.LocalValues.Material);
            Assert.True(result.LocalValues.Supported);
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
                Tags: ["32mm", "metal"],
                Category: "miniature",
                Type: "whole",
                Material: "resin",
                Supported: true,
                RaftHeightMm: null));

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
            Assert.Contains("tags:", yamlText);
            Assert.Contains("- 32mm", yamlText);
            Assert.Contains("- metal", yamlText);
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

    [Fact]
    public async Task ScanAndCacheSingleAsync_IndexesNonGeometryFile_WithoutGeometryArtifacts()
    {
        var dbFactory = CreateFactory(nameof(ScanAndCacheSingleAsync_IndexesNonGeometryFile_WithoutGeometryArtifacts));
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);

        var ctbPath = Path.Combine(modelsRoot, "sample.ctb");
        await File.WriteAllBytesAsync(ctbPath, [0x43, 0x54, 0x42, 0x00, 0x01, 0x02]);

        try
        {
            var sut = CreateSut(CreateConfiguration(modelsRoot), dbFactory);

            var indexed = await sut.ScanAndCacheSingleAsync("sample.ctb");
            Assert.True(indexed);

            await using var db = await dbFactory.CreateDbContextAsync();
            var model = await db.Models.SingleAsync(m => m.FileName == "sample.ctb");

            Assert.Equal("ctb", model.FileType);
            Assert.Null(model.PreviewImagePath);
            Assert.Null(model.PreviewGenerationVersion);
            Assert.Null(model.ConvexHullCoordinates);
            Assert.Null(model.ConcaveHullCoordinates);
            Assert.Null(model.ConvexSansRaftHullCoordinates);
            Assert.Null(model.ScanConfigChecksum);
            Assert.Null(model.GeometryCalculatedAt);

            // Re-index should no-op for unchanged non-geometry files.
            var indexedAgain = await sut.ScanAndCacheSingleAsync("sample.ctb");
            Assert.False(indexedAgain);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Fact]
    public void ModelLevelRaftOverride_ChangesExpectedChecksum_AndFlagsHullStale()
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);

        try
        {
            var fullPath = Path.Combine(modelsRoot, "dragon.stl");
            File.WriteAllText(fullPath, "solid dragon\nendsolid dragon\n");

            const float defaultRaft = 2f;
            const float overrideRaft = 7.5f;
            var directory = "";

            var directoryConfigs = new Dictionary<string, DirectoryConfig>
            {
                [""] = new DirectoryConfig
                {
                    DirectoryPath = "",
                    RawModelMetadataJson =
                        $$"""
                        {
                          "dragon.stl": {
                            "raftHeightMm": {{overrideRaft.ToString(System.Globalization.CultureInfo.InvariantCulture)}}
                          }
                        }
                        """,
                    RaftHeightMm = defaultRaft,
                }
            };

            var expectedRaft = InvokeResolveRaftHeightMmForModel(
                fullPath,
                directory,
                directoryConfigs,
                defaultRaft);

            Assert.Equal(overrideRaft, expectedRaft);

            var storedChecksum = ScanConfig.Compute(defaultRaft);
            Assert.True(InvokeNeedsHullRegeneration(storedChecksum, expectedRaft));
            Assert.False(InvokeNeedsHullRegeneration(ScanConfig.Compute(overrideRaft), expectedRaft));
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Fact]
    public async Task ScanAndCacheAsync_IndexesNonGeometryFiles_WithConfiguredParallelWorkers()
    {
        var dbFactory = CreateFactory(nameof(ScanAndCacheAsync_IndexesNonGeometryFiles_WithConfiguredParallelWorkers));
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);

        try
        {
            await File.WriteAllBytesAsync(Path.Combine(modelsRoot, "alpha.ctb"), [0x43, 0x54, 0x42, 0x01]);
            await File.WriteAllBytesAsync(Path.Combine(modelsRoot, "beta.ctb"), [0x43, 0x54, 0x42, 0x02]);
            await File.WriteAllBytesAsync(Path.Combine(modelsRoot, "gamma.ctb"), [0x43, 0x54, 0x42, 0x03]);

            var sut = CreateSut(CreateConfiguration(modelsRoot, fileScanThreads: 4), dbFactory);

            var indexedCount = await sut.ScanAndCacheAsync();

            Assert.Equal(3, indexedCount);

            await using var db = await dbFactory.CreateDbContextAsync();
            var models = await db.Models.OrderBy(m => m.FileName).ToListAsync();
            Assert.Collection(
                models,
                model => Assert.Equal("alpha.ctb", model.FileName),
                model => Assert.Equal("beta.ctb", model.FileName),
                model => Assert.Equal("gamma.ctb", model.FileName));
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Theory]
    [InlineData(null, 10)]
    [InlineData(0, 10)]
    [InlineData(-3, 10)]
    [InlineData(12, 12)]
    public void ResolveFileScanThreads_UsesConfiguredValueOrDefault(int? configuredThreads, int expectedThreads)
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);

        try
        {
            var configuration = CreateConfiguration(modelsRoot, configuredThreads);
            var method = typeof(ModelService).GetMethod("ResolveFileScanThreads", BindingFlags.NonPublic | BindingFlags.Static);

            Assert.NotNull(method);

            var result = (int?)method!.Invoke(null, [configuration]);

            Assert.Equal(expectedThreads, result);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    private static float InvokeResolveRaftHeightMmForModel(
        string fullFilePath,
        string directory,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        float defaultRaftHeightMm)
    {
        var method = typeof(ModelService).GetMethod(
            "ResolveRaftHeightMmForModel",
            BindingFlags.NonPublic | BindingFlags.Static);
        Assert.NotNull(method);

        var result = method!.Invoke(null, [fullFilePath, directory, directoryConfigs, defaultRaftHeightMm]);
        Assert.IsType<float>(result);
        return (float)result!;
    }

    private static bool InvokeNeedsHullRegeneration(string? storedChecksum, float expectedRaftHeightMm)
    {
        var method = typeof(ModelService).GetMethod(
            "NeedsHullRegeneration",
            BindingFlags.NonPublic | BindingFlags.Static);
        Assert.NotNull(method);

        var result = method!.Invoke(null, [storedChecksum, expectedRaftHeightMm]);
        Assert.IsType<bool>(result);
        return (bool)result!;
    }

    private static async Task<string> ComputeChecksumAsync(string filePath)
    {
        await using var stream = File.OpenRead(filePath);
        var hash = await SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
