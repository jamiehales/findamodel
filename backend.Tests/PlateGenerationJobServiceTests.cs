using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class PlateGenerationJobServiceTests
{
    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);
        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
            => Task.FromResult(CreateDbContext());
    }

    private static IDbContextFactory<ModelCacheContext> CreateFactory(string dbName)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;

        using var db = new ModelCacheContext(options);
        db.AppConfigs.Add(new AppConfig
        {
            Id = 1,
            TagGenerationEnabled = false,
            MinimumPreviewGenerationVersion = ModelPreviewService.CurrentPreviewGenerationVersion,
        });
        db.PrinterConfigs.Add(new PrinterConfig
        {
            Id = Guid.NewGuid(),
            Name = "Default test printer",
            BedWidthMm = 228,
            BedDepthMm = 128,
            PixelWidth = 320,
            PixelHeight = 180,
            IsBuiltIn = true,
            IsDefault = true,
        });
        db.SaveChanges();

        return new InMemoryDbContextFactory(options);
    }

    private static IConfiguration CreateConfiguration(string modelsRoot)
    {
        return new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Models:DirectoryPath"] = modelsRoot,
            })
            .Build();
    }

    private static ModelService CreateModelService(
        IConfiguration configuration,
        IDbContextFactory<ModelCacheContext> dbFactory)
    {
        var loggerFactory = NullLoggerFactory.Instance;
        var configReader = new DirectoryConfigReader(loggerFactory);
        var metadataConfigService = new MetadataConfigService(configuration, loggerFactory, dbFactory, configReader);
        var appConfigService = new AppConfigService(dbFactory, configuration);

        return new ModelService(
            configuration,
            loggerFactory,
            dbFactory,
            loaderService: null!,
            previewService: null!,
            hullCalculationService: null!,
            metadataConfigService,
            appConfigService,
            tagGenerationService: null!);
    }

    private static async Task WaitForCompletionAsync(PlateGenerationJobService sut, Guid jobId)
    {
        for (var i = 0; i < 200; i++)
        {
            var job = sut.GetJob(jobId);
            Assert.NotNull(job);

            if (job!.Status is "completed" or "failed")
                return;

            await Task.Delay(25);
        }

        throw new TimeoutException("Plate generation job did not finish in time.");
    }

    private static Task WriteSimpleAsciiStlAsync(string path)
    {
        return File.WriteAllTextAsync(path, """
            solid tetrahedron
              facet normal 0 0 -1
                outer loop
                  vertex 0 0 0
                  vertex 10 0 0
                  vertex 5 0 10
                endloop
              endfacet
              facet normal 0.8944 0.4472 0
                outer loop
                  vertex 0 0 0
                  vertex 5 10 5
                  vertex 10 0 0
                endloop
              endfacet
              facet normal -0.6667 0.3333 0.6667
                outer loop
                  vertex 10 0 0
                  vertex 5 10 5
                  vertex 5 0 10
                endloop
              endfacet
              facet normal -0.6667 0.3333 -0.6667
                outer loop
                  vertex 5 0 10
                  vertex 5 10 5
                  vertex 0 0 0
                endloop
              endfacet
            endsolid tetrahedron
            """);
    }

    [Fact]
    public async Task CreateJobAsync_ReportsProgressAndProducesPlateFile()
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-plate-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);
        var dbFactory = CreateFactory(nameof(CreateJobAsync_ReportsProgressAndProducesPlateFile));

        try
        {
            var firstModelId = Guid.NewGuid();
            var secondModelId = Guid.NewGuid();
            await WriteSimpleAsciiStlAsync(Path.Combine(modelsRoot, "first.stl"));
            await WriteSimpleAsciiStlAsync(Path.Combine(modelsRoot, "second.stl"));

            await using (var db = await dbFactory.CreateDbContextAsync())
            {
                db.Models.AddRange(
                    new CachedModel
                    {
                        Id = firstModelId,
                        FileName = "first.stl",
                        Directory = "",
                        FileType = "stl",
                        Checksum = "a",
                        FileSize = 1,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                    },
                    new CachedModel
                    {
                        Id = secondModelId,
                        FileName = "second.stl",
                        Directory = "",
                        FileType = "stl",
                        Checksum = "b",
                        FileSize = 1,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                    });
                await db.SaveChangesAsync();
            }

            var configuration = CreateConfiguration(modelsRoot);
            var plateExportService = new PlateExportService(
                CreateModelService(configuration, dbFactory),
                new ModelLoaderService(NullLoggerFactory.Instance),
                new ModelSaverService(),
                new PlateSliceRasterService(
                [
                    new MeshIntersectionSliceBitmapGenerator(),
                    new OrthographicProjectionSliceBitmapGenerator(),
                ]),
                new PrinterService(dbFactory),
                configuration);
            var sut = new PlateGenerationJobService(plateExportService, NullLoggerFactory.Instance);

            var started = await sut.CreateJobAsync(
                new GeneratePlateRequest(
                [
                    new PlacementDto(firstModelId.ToString(), 0, 0, 0, 0),
                    new PlacementDto(firstModelId.ToString(), 1, 20, 0, 0),
                    new PlacementDto(secondModelId.ToString(), 0, 40, 0, 0),
                ],
                "3mf"));

            Assert.Equal("queued", started.Status);
            Assert.Equal(3, started.TotalEntries);

            await WaitForCompletionAsync(sut, started.JobId);

            var completed = sut.GetJob(started.JobId);
            Assert.NotNull(completed);
            Assert.Equal("completed", completed!.Status);
            Assert.Equal(3, completed.CompletedEntries);
            Assert.Equal(100, completed.ProgressPercent);
            Assert.Empty(completed.SkippedModels);

            var file = sut.GetCompletedJobFile(started.JobId);
            Assert.NotNull(file);
            Assert.Equal("plate.3mf", file!.Value.FileName);
            Assert.True(File.Exists(file.Value.Path));

            await sut.RemoveJobAsync(started.JobId);
            Assert.False(File.Exists(file.Value.Path));
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Fact]
    public async Task CreateJobAsync_PreservesSkippedModelWarning()
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-plate-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);
        var dbFactory = CreateFactory(nameof(CreateJobAsync_PreservesSkippedModelWarning));

        try
        {
            var printableModelId = Guid.NewGuid();
            var skippedModelId = Guid.NewGuid();
            await WriteSimpleAsciiStlAsync(Path.Combine(modelsRoot, "printable.stl"));

            await using (var db = await dbFactory.CreateDbContextAsync())
            {
                db.Models.AddRange(
                    new CachedModel
                    {
                        Id = printableModelId,
                        FileName = "printable.stl",
                        Directory = "",
                        FileType = "stl",
                        Checksum = "c",
                        FileSize = 1,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                    },
                    new CachedModel
                    {
                        Id = skippedModelId,
                        FileName = "skipme.lys",
                        Directory = "",
                        FileType = "lys",
                        Checksum = "d",
                        FileSize = 1,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                    });
                await db.SaveChangesAsync();
            }

            var configuration = CreateConfiguration(modelsRoot);
            var plateExportService = new PlateExportService(
                CreateModelService(configuration, dbFactory),
                new ModelLoaderService(NullLoggerFactory.Instance),
                new ModelSaverService(),
                new PlateSliceRasterService(
                [
                    new MeshIntersectionSliceBitmapGenerator(),
                    new OrthographicProjectionSliceBitmapGenerator(),
                ]),
                new PrinterService(dbFactory),
                configuration);
            var sut = new PlateGenerationJobService(plateExportService, NullLoggerFactory.Instance);

            var started = await sut.CreateJobAsync(
                new GeneratePlateRequest(
                [
                    new PlacementDto(skippedModelId.ToString(), 0, 0, 0, 0),
                    new PlacementDto(printableModelId.ToString(), 0, 20, 0, 0),
                ],
                "stl"));

            await WaitForCompletionAsync(sut, started.JobId);

            var completed = sut.GetJob(started.JobId);
            Assert.NotNull(completed);
            Assert.Equal("completed", completed!.Status);
            Assert.NotNull(completed.Warning);
            Assert.Contains("skipme.lys", completed.SkippedModels);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Theory]
    [InlineData("pngzip_mesh", "plate-slices-mesh.zip")]
    [InlineData("pngzip_orthographic", "plate-slices-orthographic.zip")]
    public async Task CreateJobAsync_CreatesPngSliceArchive(string format, string expectedFileName)
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-plate-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);
        var dbFactory = CreateFactory($"{nameof(CreateJobAsync_CreatesPngSliceArchive)}-{format}");

        try
        {
            var modelId = Guid.NewGuid();
            await WriteSimpleAsciiStlAsync(Path.Combine(modelsRoot, "sliceable.stl"));

            await using (var db = await dbFactory.CreateDbContextAsync())
            {
                db.Models.Add(new CachedModel
                {
                    Id = modelId,
                    FileName = "sliceable.stl",
                    Directory = "",
                    FileType = "stl",
                    Checksum = "slice",
                    FileSize = 1,
                    FileModifiedAt = DateTime.UtcNow,
                    CachedAt = DateTime.UtcNow,
                });
                await db.SaveChangesAsync();
            }

            var configuration = CreateConfiguration(modelsRoot);
            var plateExportService = new PlateExportService(
                CreateModelService(configuration, dbFactory),
                new ModelLoaderService(NullLoggerFactory.Instance),
                new ModelSaverService(),
                new PlateSliceRasterService(
                [
                    new MeshIntersectionSliceBitmapGenerator(),
                    new OrthographicProjectionSliceBitmapGenerator(),
                ]),
                new PrinterService(dbFactory),
                configuration);
            var sut = new PlateGenerationJobService(plateExportService, NullLoggerFactory.Instance);

            var printer = await new PrinterService(dbFactory).GetDefaultAsync();
            Assert.NotNull(printer);

            var started = await sut.CreateJobAsync(
                new GeneratePlateRequest(
                [
                    new PlacementDto(modelId.ToString(), 0, printer!.BedWidthMm / 2f, printer.BedDepthMm / 2f, 0),
                ],
                format));

            await WaitForCompletionAsync(sut, started.JobId);

            var completed = sut.GetJob(started.JobId);
            Assert.NotNull(completed);
            Assert.Equal("completed", completed!.Status);
            Assert.Equal(expectedFileName, completed.FileName);

            var file = sut.GetCompletedJobFile(started.JobId);
            Assert.NotNull(file);
            using var stream = File.OpenRead(file!.Value.Path);
            using var archive = new System.IO.Compression.ZipArchive(stream, System.IO.Compression.ZipArchiveMode.Read);
            Assert.NotNull(archive.GetEntry("manifest.json"));
            var pngEntries = archive.Entries
                .Where(e => e.FullName.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
                .ToList();
            Assert.NotEmpty(pngEntries);

            var hasLitLayer = false;
            foreach (var pngEntry in pngEntries)
            {
                using var pngStream = pngEntry.Open();
                using var image = SixLabors.ImageSharp.Image.Load<SixLabors.ImageSharp.PixelFormats.L8>(pngStream);
                var litPixels = 0;
                image.ProcessPixelRows(accessor =>
                {
                    for (var y = 0; y < image.Height; y++)
                    {
                        foreach (var pixel in accessor.GetRowSpan(y))
                        {
                            if (pixel.PackedValue > 0)
                                litPixels++;
                        }
                    }
                });

                if (litPixels > 0)
                {
                    hasLitLayer = true;
                    break;
                }
            }

            Assert.True(hasLitLayer, "Expected at least one lit pixel in the exported slice stack.");
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }
}
