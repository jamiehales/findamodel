using System.IO.Compression;
using System.Text.Json;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
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

    private static Task WriteRepeatedAsciiStlAsync(string path, int repeatCount)
    {
        var sb = new System.Text.StringBuilder();
        sb.AppendLine("solid repeated");
        for (var i = 0; i < repeatCount; i++)
        {
            sb.AppendLine("  facet normal 0 0 -1");
            sb.AppendLine("    outer loop");
            sb.AppendLine($"      vertex {i % 25} 0 0");
            sb.AppendLine($"      vertex {(i % 25) + 1} 0 0");
            sb.AppendLine($"      vertex {(i % 25) + 0.5} 0 10");
            sb.AppendLine("    endloop");
            sb.AppendLine("  endfacet");

            sb.AppendLine("  facet normal 0.8944 0.4472 0");
            sb.AppendLine("    outer loop");
            sb.AppendLine($"      vertex {i % 25} 0 0");
            sb.AppendLine($"      vertex {(i % 25) + 0.5} 10 5");
            sb.AppendLine($"      vertex {(i % 25) + 1} 0 0");
            sb.AppendLine("    endloop");
            sb.AppendLine("  endfacet");
        }

        sb.AppendLine("endsolid repeated");
        return File.WriteAllTextAsync(path, sb.ToString());
    }

    private static IReadOnlyList<IReadOnlyList<Triangle3D>> BuildPlacedGroups(
        IReadOnlyList<Triangle3D> triangles,
        IReadOnlyList<PlacementDto> placements,
        PrinterConfigDto printer)
    {
        var groups = new List<IReadOnlyList<Triangle3D>>(placements.Count);
        foreach (var placement in placements)
        {
            var offsetX = (float)(placement.XMm - (printer.BedWidthMm * 0.5f));
            var offsetZ = (float)((printer.BedDepthMm * 0.5f) - placement.YMm);
            var sinA = MathF.Sin((float)placement.AngleRad);
            var cosA = MathF.Cos((float)placement.AngleRad);

            static Vec3 Rotate(Vec3 v, float sin, float cos)
                => new(v.X * cos - v.Z * sin, v.Y, v.X * sin + v.Z * cos);

            groups.Add(triangles
                .Select(triangle =>
                {
                    var v0 = Rotate(triangle.V0, sinA, cosA);
                    var v1 = Rotate(triangle.V1, sinA, cosA);
                    var v2 = Rotate(triangle.V2, sinA, cosA);
                    var normal = Rotate(triangle.Normal, sinA, cosA);
                    return new Triangle3D(
                        new Vec3(v0.X + offsetX, v0.Y, v0.Z + offsetZ),
                        new Vec3(v1.X + offsetX, v1.Y, v1.Z + offsetZ),
                        new Vec3(v2.X + offsetX, v2.Y, v2.Z + offsetZ),
                        normal);
                })
                .ToArray());
        }

        return groups;
    }

    private static IReadOnlyList<IReadOnlyList<Triangle3D>> BuildPlacedGroups(
        IReadOnlyDictionary<Guid, LoadedGeometry> geometryByModelId,
        IReadOnlyList<PlacementDto> placements,
        PrinterConfigDto printer)
    {
        var groups = new List<IReadOnlyList<Triangle3D>>(placements.Count);
        foreach (var placement in placements)
        {
            Assert.True(Guid.TryParse(placement.ModelId, out var modelId));
            Assert.True(geometryByModelId.TryGetValue(modelId, out var geometry));
            groups.AddRange(BuildPlacedGroups(geometry!.Triangles, [placement], printer));
        }

        return groups;
    }

    private static void AssertZipPngStacksEqual(byte[] expectedZip, byte[] actualZip)
    {
        using var expectedStream = new MemoryStream(expectedZip);
        using var actualStream = new MemoryStream(actualZip);
        using var expectedArchive = new ZipArchive(expectedStream, ZipArchiveMode.Read);
        using var actualArchive = new ZipArchive(actualStream, ZipArchiveMode.Read);

        var expectedPngs = expectedArchive.Entries
            .Where(entry => entry.FullName.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
            .OrderBy(entry => entry.FullName, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        var actualPngs = actualArchive.Entries
            .Where(entry => entry.FullName.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
            .OrderBy(entry => entry.FullName, StringComparer.OrdinalIgnoreCase)
            .ToArray();

        Assert.Equal(expectedPngs.Length, actualPngs.Length);

        for (var i = 0; i < expectedPngs.Length; i++)
        {
            Assert.Equal(expectedPngs[i].FullName, actualPngs[i].FullName);
            using var expectedPngStream = expectedPngs[i].Open();
            using var actualPngStream = actualPngs[i].Open();
            using var expectedImage = Image.Load<L8>(expectedPngStream);
            using var actualImage = Image.Load<L8>(actualPngStream);
            Assert.Equal(expectedImage.Width, actualImage.Width);
            Assert.Equal(expectedImage.Height, actualImage.Height);

            var expectedPixels = new byte[expectedImage.Width * expectedImage.Height];
            var actualPixels = new byte[actualImage.Width * actualImage.Height];

            expectedImage.ProcessPixelRows(accessor =>
            {
                for (var y = 0; y < expectedImage.Height; y++)
                {
                    var row = accessor.GetRowSpan(y);
                    for (var x = 0; x < row.Length; x++)
                        expectedPixels[(y * expectedImage.Width) + x] = row[x].PackedValue;
                }
            });

            actualImage.ProcessPixelRows(accessor =>
            {
                for (var y = 0; y < actualImage.Height; y++)
                {
                    var row = accessor.GetRowSpan(y);
                    for (var x = 0; x < row.Length; x++)
                        actualPixels[(y * actualImage.Width) + x] = row[x].PackedValue;
                }
            });

            Assert.Equal(expectedPixels, actualPixels);
        }
    }

    private static void AssertZipLayersMatchRenderedBitmaps(
        byte[] actualZip,
        PlateSliceRasterService sliceRasterService,
        IReadOnlyList<IReadOnlyList<Triangle3D>> groups,
        PrinterConfigDto printer,
        PngSliceExportMethod method,
        IEnumerable<int> layerIndexes)
    {
        using var actualStream = new MemoryStream(actualZip);
        using var actualArchive = new ZipArchive(actualStream, ZipArchiveMode.Read);

        foreach (var layerIndex in layerIndexes)
        {
            var entry = actualArchive.GetEntry($"slices/layer_{layerIndex:D5}.png")
                ?? actualArchive.GetEntry($"layer_{layerIndex:D5}.png");
            Assert.NotNull(entry);

            using var pngStream = entry!.Open();
            using var image = Image.Load<L8>(pngStream);
            var actualPixels = new byte[image.Width * image.Height];
            image.ProcessPixelRows(accessor =>
            {
                for (var y = 0; y < image.Height; y++)
                {
                    var row = accessor.GetRowSpan(y);
                    for (var x = 0; x < row.Length; x++)
                        actualPixels[(y * image.Width) + x] = row[x].PackedValue;
                }
            });

            var layerHeightMm = PlateSliceRasterService.DefaultLayerHeightMm;
            var sliceHeightMm = (layerIndex * layerHeightMm) + (layerHeightMm * 0.5f);
            var expectedBitmap = sliceRasterService.RenderLayerBitmap(
                groups,
                sliceHeightMm,
                printer.BedWidthMm,
                printer.BedDepthMm,
                printer.PixelWidth,
                printer.PixelHeight,
                method,
                layerHeightMm);

            Assert.Equal(expectedBitmap.Pixels, actualPixels);
        }
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
            Assert.True(completed.TotalEntries > 1, "Expected PNG slice export to report multi-step slice progress.");
            Assert.Equal(completed.TotalEntries, completed.CompletedEntries);

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

    [Theory]
    [InlineData("pngzip_mesh", PngSliceExportMethod.MeshIntersection)]
    [InlineData("pngzip_orthographic", PngSliceExportMethod.OrthographicProjection)]
    public async Task GeneratePlateAsync_PngSliceExport_MatchesGroupedPlacementComposition(string format, PngSliceExportMethod method)
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-plate-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);
        var dbFactory = CreateFactory($"{nameof(GeneratePlateAsync_PngSliceExport_MatchesGroupedPlacementComposition)}-{format}");

        try
        {
            var modelId = Guid.NewGuid();
            var stlPath = Path.Combine(modelsRoot, "sliceable.stl");
            await WriteSimpleAsciiStlAsync(stlPath);

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
            var sliceRasterService = new PlateSliceRasterService(
            [
                new MeshIntersectionSliceBitmapGenerator(),
                new OrthographicProjectionSliceBitmapGenerator(),
            ]);
            var printerService = new PrinterService(dbFactory);
            var plateExportService = new PlateExportService(
                CreateModelService(configuration, dbFactory),
                new ModelLoaderService(NullLoggerFactory.Instance),
                new ModelSaverService(),
                sliceRasterService,
                printerService,
                configuration);

            var printer = await printerService.GetDefaultAsync();
            Assert.NotNull(printer);

            var request = new GeneratePlateRequest(
            [
                new PlacementDto(modelId.ToString(), 0, printer!.BedWidthMm * 0.35f, printer.BedDepthMm * 0.35f, 0.31),
                new PlacementDto(modelId.ToString(), 1, printer.BedWidthMm * 0.67f, printer.BedDepthMm * 0.56f, -0.44),
            ],
            format,
            printer!.Id);

            var result = await plateExportService.GeneratePlateAsync(request);
            Assert.NotEmpty(result.Content);

            var geometry = await new ModelLoaderService(NullLoggerFactory.Instance).LoadModelAsync(stlPath, "stl");
            Assert.NotNull(geometry);

            var expectedGroups = BuildPlacedGroups(geometry!.Triangles, request.Placements, printer);
            var expected = sliceRasterService.GenerateSliceArchive(
                expectedGroups,
                printer.BedWidthMm,
                printer.BedDepthMm,
                printer.PixelWidth,
                printer.PixelHeight,
                method);

            AssertZipPngStacksEqual(expected, result.Content);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }

    [Theory]
    [InlineData("pngzip_mesh", PngSliceExportMethod.MeshIntersection)]
    [InlineData("pngzip_orthographic", PngSliceExportMethod.OrthographicProjection)]
    public async Task GeneratePlateAsync_PngSliceExport_WithLocalPayload_MatchesGroupedPlacementComposition(string format, PngSliceExportMethod method)
    {
        var payloadPath = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "backend.Tests", "TestData", "plate-export-repro-payload.json"));
        var dbPath = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "debug", "data", "findamodel.db"));
        if (!File.Exists(payloadPath) || !File.Exists(dbPath))
            return;

        var payload = JsonSerializer.Deserialize<GeneratePlateRequest>(await File.ReadAllTextAsync(payloadPath), new JsonSerializerOptions
        {
            PropertyNameCaseInsensitive = true,
        });

        Assert.NotNull(payload);
        Assert.NotEmpty(payload!.Placements);

        var sourceOptions = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseSqlite($"Data Source={dbPath}")
            .Options;

        await using var sourceDb = new ModelCacheContext(sourceOptions);
        var modelsRoot = await sourceDb.AppConfigs.Select(config => config.ModelsDirectoryPath).FirstOrDefaultAsync();
        if (string.IsNullOrWhiteSpace(modelsRoot) || !Directory.Exists(modelsRoot))
            return;

        var modelIds = payload.Placements
            .Select(placement => Guid.Parse(placement.ModelId))
            .Distinct()
            .ToArray();
        var sourceModels = await sourceDb.Models
            .Where(model => modelIds.Contains(model.Id))
            .ToListAsync();
        if (sourceModels.Count != modelIds.Length)
            return;

        var sourcePrinter = payload.PrinterConfigId.HasValue
            ? await sourceDb.PrinterConfigs.FirstOrDefaultAsync(printer => printer.Id == payload.PrinterConfigId.Value)
            : await sourceDb.PrinterConfigs.OrderByDescending(printer => printer.IsDefault).FirstOrDefaultAsync();
        if (sourcePrinter is null)
            return;

        var dbFactory = CreateFactory($"{nameof(GeneratePlateAsync_PngSliceExport_WithLocalPayload_MatchesGroupedPlacementComposition)}-{format}");
        var payloadPrinterId = Guid.NewGuid();
        await using (var targetDb = await dbFactory.CreateDbContextAsync())
        {
            targetDb.Models.AddRange(sourceModels.Select(model => new CachedModel
            {
                Id = model.Id,
                FileName = model.FileName,
                Directory = model.Directory,
                FileType = model.FileType,
                Checksum = model.Checksum,
                FileSize = model.FileSize,
                FileModifiedAt = model.FileModifiedAt,
                CachedAt = model.CachedAt,
            }));

            targetDb.PrinterConfigs.Add(new PrinterConfig
            {
                Id = payloadPrinterId,
                Name = $"Payload regression printer {format}",
                BedWidthMm = sourcePrinter!.BedWidthMm,
                BedDepthMm = sourcePrinter.BedDepthMm,
                PixelWidth = 960,
                PixelHeight = 540,
                IsBuiltIn = false,
                IsDefault = false,
            });

            await targetDb.SaveChangesAsync();
        }

        var configuration = CreateConfiguration(modelsRoot!);
        var sliceRasterService = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            new OrthographicProjectionSliceBitmapGenerator(),
        ]);
        var printerService = new PrinterService(dbFactory);
        var plateExportService = new PlateExportService(
            CreateModelService(configuration, dbFactory),
            new ModelLoaderService(NullLoggerFactory.Instance),
            new ModelSaverService(),
            sliceRasterService,
            printerService,
            configuration);

        var printer = await printerService.GetByIdAsync(payloadPrinterId);
        Assert.NotNull(printer);

        var request = payload with { Format = format, PrinterConfigId = payloadPrinterId };
        var result = await plateExportService.GeneratePlateAsync(request);
        Assert.NotEmpty(result.Content);

        var geometryByModelId = new Dictionary<Guid, LoadedGeometry>();
        var loader = new ModelLoaderService(NullLoggerFactory.Instance);
        foreach (var model in sourceModels)
        {
            var relativeDirectory = model.Directory.Replace('/', Path.DirectorySeparatorChar);
            var modelPath = Path.Combine(modelsRoot!, relativeDirectory, model.FileName);
            if (!File.Exists(modelPath))
                return;

            var geometry = await loader.LoadModelAsync(modelPath, model.FileType);
            if (geometry is null)
                return;
            geometryByModelId[model.Id] = geometry!;
        }

        var expectedGroups = BuildPlacedGroups(geometryByModelId, request.Placements, printer!);
        AssertZipLayersMatchRenderedBitmaps(
            result.Content,
            sliceRasterService,
            expectedGroups,
            printer,
            method,
            Enumerable.Range(0, 11));
    }

    [Fact]
    public async Task CreateJobAsync_QueuesLaterJobsWhileSliceExportIsRunning()
    {
        var modelsRoot = Path.Combine(Path.GetTempPath(), $"findamodel-plate-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(modelsRoot);
        var dbFactory = CreateFactory(nameof(CreateJobAsync_QueuesLaterJobsWhileSliceExportIsRunning));

        try
        {
            var modelId = Guid.NewGuid();
            await WriteRepeatedAsciiStlAsync(Path.Combine(modelsRoot, "slow.stl"), repeatCount: 3000);

            await using (var db = await dbFactory.CreateDbContextAsync())
            {
                db.Models.Add(new CachedModel
                {
                    Id = modelId,
                    FileName = "slow.stl",
                    Directory = "",
                    FileType = "stl",
                    Checksum = "queue",
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

            var first = await sut.CreateJobAsync(
                new GeneratePlateRequest(
                [
                    new PlacementDto(modelId.ToString(), 0, printer!.BedWidthMm / 2f, printer.BedDepthMm / 2f, 0),
                ],
                "pngzip_mesh"));

            var second = await sut.CreateJobAsync(
                new GeneratePlateRequest(
                [
                    new PlacementDto(modelId.ToString(), 1, printer.BedWidthMm / 2f, printer.BedDepthMm / 2f, 0),
                ],
                "pngzip_mesh"));

            var observedFirstRunning = false;
            for (var i = 0; i < 200; i++)
            {
                var firstState = sut.GetJob(first.JobId);
                var secondState = sut.GetJob(second.JobId);
                Assert.NotNull(firstState);
                Assert.NotNull(secondState);

                if (firstState!.Status == "running")
                {
                    observedFirstRunning = true;
                    Assert.Equal("queued", secondState!.Status);
                    break;
                }

                if (firstState.Status == "completed")
                    break;

                await Task.Delay(10);
            }

            Assert.True(observedFirstRunning, "Expected the first slice job to enter the running state.");

            await WaitForCompletionAsync(sut, first.JobId);
            await WaitForCompletionAsync(sut, second.JobId);
        }
        finally
        {
            if (Directory.Exists(modelsRoot))
                Directory.Delete(modelsRoot, recursive: true);
        }
    }
}
