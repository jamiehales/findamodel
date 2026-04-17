using System.IO.Compression;
using System.Text.Json;
using findamodel.Data;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging.Abstractions;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
using Xunit;

namespace findamodel.Tests;

public class PlateSliceRasterServiceTests
{
    private static IReadOnlyList<Triangle3D> CreateTetrahedron() =>
    [
        new(new Vec3(-2, 0, -2), new Vec3(2, 0, -2), new Vec3(0, 0, 2), Vec3.Up),
        new(new Vec3(-2, 0, -2), new Vec3(0, 4, 0), new Vec3(2, 0, -2), Vec3.Up),
        new(new Vec3(2, 0, -2), new Vec3(0, 4, 0), new Vec3(0, 0, 2), Vec3.Up),
        new(new Vec3(0, 0, 2), new Vec3(0, 4, 0), new Vec3(-2, 0, -2), Vec3.Up),
    ];

    [Fact]
    public void SliceBitmapCleanup_RemovesDiagonalOnlyHorizontalSmear()
    {
        var bitmap = new SliceBitmap(8, 8);
        bitmap.SetPixel(2, 3, 255);
        bitmap.SetPixel(3, 3, 255);
        bitmap.SetPixel(4, 3, 255);
        bitmap.SetPixel(1, 2, 255);
        bitmap.SetPixel(5, 4, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        Assert.Equal(0, bitmap.GetPixel(2, 3));
        Assert.Equal(0, bitmap.GetPixel(3, 3));
        Assert.Equal(0, bitmap.GetPixel(4, 3));
    }

    [Fact]
    public void SliceBitmapCleanup_PreservesSupportedSlopedBand()
    {
        var bitmap = new SliceBitmap(8, 8);
        bitmap.SetPixel(1, 2, 255);
        bitmap.SetPixel(2, 2, 255);

        bitmap.SetPixel(2, 3, 255);
        bitmap.SetPixel(3, 3, 255);
        bitmap.SetPixel(4, 3, 255);

        bitmap.SetPixel(4, 4, 255);
        bitmap.SetPixel(5, 4, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        Assert.Equal(255, bitmap.GetPixel(2, 3));
        Assert.Equal(255, bitmap.GetPixel(3, 3));
        Assert.Equal(255, bitmap.GetPixel(4, 3));
    }

    [Fact]
    public void SliceBitmapCleanup_PreservesDiagonallySupportedEdgeRun()
    {
        var bitmap = new SliceBitmap(12, 8);
        bitmap.SetPixel(1, 2, 255);
        bitmap.SetPixel(2, 2, 255);
        bitmap.SetPixel(3, 2, 255);

        bitmap.SetPixel(4, 3, 255);
        bitmap.SetPixel(5, 3, 255);
        bitmap.SetPixel(6, 3, 255);
        bitmap.SetPixel(7, 3, 255);

        bitmap.SetPixel(8, 4, 255);
        bitmap.SetPixel(9, 4, 255);
        bitmap.SetPixel(10, 4, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        Assert.Equal(255, bitmap.GetPixel(4, 3));
        Assert.Equal(255, bitmap.GetPixel(5, 3));
        Assert.Equal(255, bitmap.GetPixel(6, 3));
        Assert.Equal(255, bitmap.GetPixel(7, 3));
    }

    [Fact]
    public void SliceBitmapCleanup_FillsSingleMissingInteriorRow()
    {
        var bitmap = new SliceBitmap(10, 10);
        for (var x = 2; x <= 7; x++)
        {
            bitmap.SetPixel(x, 3, 255);
            bitmap.SetPixel(x, 4, 255);
            bitmap.SetPixel(x, 6, 255);
            bitmap.SetPixel(x, 7, 255);
        }

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var x = 2; x <= 7; x++)
            Assert.Equal(255, bitmap.GetPixel(x, 5));
    }

    [Fact]
    public void SliceBitmapCleanup_PreservesWideSpanWithoutExactVerticalOverlap()
    {
        var bitmap = new SliceBitmap(20, 10);
        for (var x = 2; x <= 6; x++)
            bitmap.SetPixel(x, 2, 255);

        for (var x = 7; x <= 13; x++)
            bitmap.SetPixel(x, 3, 255);

        for (var x = 14; x <= 18; x++)
            bitmap.SetPixel(x, 4, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var x = 7; x <= 13; x++)
            Assert.Equal(255, bitmap.GetPixel(x, 3));
    }

    [Fact]
    public void SliceBitmapCleanup_FillsShortEdgeNotchBetweenSupportedRows()
    {
        var bitmap = new SliceBitmap(16, 8);
        for (var x = 3; x <= 12; x++)
        {
            bitmap.SetPixel(x, 2, 255);
            bitmap.SetPixel(x, 4, 255);
        }

        for (var x = 6; x <= 12; x++)
            bitmap.SetPixel(x, 3, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var x = 3; x <= 12; x++)
            Assert.Equal(255, bitmap.GetPixel(x, 3));
    }

    [Fact]
    public void SliceBitmapCleanup_FillsTwoRowEdgeNotchBetweenSupportedRows()
    {
        var bitmap = new SliceBitmap(16, 10);
        for (var x = 3; x <= 12; x++)
        {
            bitmap.SetPixel(x, 2, 255);
            bitmap.SetPixel(x, 5, 255);
        }

        for (var x = 6; x <= 12; x++)
        {
            bitmap.SetPixel(x, 3, 255);
            bitmap.SetPixel(x, 4, 255);
        }

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var y = 3; y <= 4; y++)
        {
            for (var x = 3; x <= 12; x++)
                Assert.Equal(255, bitmap.GetPixel(x, y));
        }
    }

    [Fact]
    public void SliceBitmapCleanup_RemovesDetachedWideHorizontalLine()
    {
        var bitmap = new SliceBitmap(40, 20);
        for (var y = 4; y <= 14; y++)
        {
            for (var x = 8; x <= 28; x++)
                bitmap.SetPixel(x, y, 255);
        }

        for (var x = 12; x <= 24; x++)
            bitmap.SetPixel(x, 17, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var x = 12; x <= 24; x++)
            Assert.Equal(0, bitmap.GetPixel(x, 17));

        Assert.Equal(255, bitmap.GetPixel(16, 10));
    }

    [Fact]
    public void SliceBitmapCleanup_RemovesDetachedSlashCluster()
    {
        var bitmap = new SliceBitmap(50, 30);
        for (var y = 5; y <= 22; y++)
        {
            for (var x = 6; x <= 30; x++)
                bitmap.SetPixel(x, y, 255);
        }

        bitmap.SetPixel(36, 10, 255);
        bitmap.SetPixel(37, 10, 255);
        bitmap.SetPixel(35, 11, 255);
        bitmap.SetPixel(36, 11, 255);
        bitmap.SetPixel(37, 11, 255);
        bitmap.SetPixel(34, 12, 255);
        bitmap.SetPixel(35, 12, 255);
        bitmap.SetPixel(36, 12, 255);
        bitmap.SetPixel(38, 9, 255);
        bitmap.SetPixel(39, 9, 255);
        bitmap.SetPixel(40, 9, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        Assert.Equal(0, bitmap.GetPixel(36, 10));
        Assert.Equal(0, bitmap.GetPixel(35, 11));
        Assert.Equal(0, bitmap.GetPixel(34, 12));
        Assert.Equal(255, bitmap.GetPixel(20, 12));
    }

    [Fact]
    public void SliceBitmapCleanup_FillsLongInteriorHorizontalBandDropout()
    {
        var bitmap = new SliceBitmap(48, 28);
        for (var y = 4; y <= 23; y++)
        {
            for (var x = 6; x <= 41; x++)
                bitmap.SetPixel(x, y, 255);
        }

        for (var x = 8; x <= 39; x++)
            bitmap.SetPixel(x, 13, 0);

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var x = 8; x <= 39; x++)
            Assert.Equal(255, bitmap.GetPixel(x, 13));
    }

    [Fact]
    public void SliceBitmapCleanup_FillsThreeRowInteriorBandDropout()
    {
        var bitmap = new SliceBitmap(48, 32);
        for (var y = 4; y <= 26; y++)
        {
            for (var x = 6; x <= 41; x++)
                bitmap.SetPixel(x, y, 255);
        }

        for (var y = 13; y <= 15; y++)
        {
            for (var x = 8; x <= 39; x++)
                bitmap.SetPixel(x, y, 0);
        }

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var y = 13; y <= 15; y++)
        {
            for (var x = 8; x <= 39; x++)
                Assert.Equal(255, bitmap.GetPixel(x, y));
        }
    }

    [Fact]
    public void SliceBitmapCleanup_FillsSparseInteriorBandWithAttachedSlashArtifact()
    {
        var bitmap = new SliceBitmap(60, 32);
        for (var y = 5; y <= 25; y++)
        {
            for (var x = 8; x <= 49; x++)
                bitmap.SetPixel(x, y, 255);
        }

        for (var x = 8; x <= 46; x++)
            bitmap.SetPixel(x, 14, 0);

        bitmap.SetPixel(38, 14, 255);
        bitmap.SetPixel(39, 14, 255);
        bitmap.SetPixel(37, 15, 255);
        bitmap.SetPixel(38, 15, 255);
        bitmap.SetPixel(39, 15, 255);

        bitmap.RemoveUnsupportedHorizontalPixels();

        for (var x = 8; x <= 46; x++)
            Assert.Equal(255, bitmap.GetPixel(x, 14));
    }

    [Fact]
    public void SliceBitmapCleanup_FillsDiagonalInteriorVoidCluster()
    {
        var bitmap = new SliceBitmap(48, 28);
        for (var y = 4; y <= 23; y++)
        {
            for (var x = 6; x <= 41; x++)
                bitmap.SetPixel(x, y, 255);
        }

        bitmap.SetPixel(27, 10, 0);
        bitmap.SetPixel(28, 10, 0);
        bitmap.SetPixel(26, 11, 0);
        bitmap.SetPixel(27, 11, 0);
        bitmap.SetPixel(28, 11, 0);
        bitmap.SetPixel(25, 12, 0);
        bitmap.SetPixel(26, 12, 0);
        bitmap.SetPixel(27, 12, 0);
        bitmap.SetPixel(29, 10, 0);
        bitmap.SetPixel(30, 10, 0);
        bitmap.SetPixel(31, 10, 0);

        bitmap.RemoveUnsupportedHorizontalPixels();

        Assert.Equal(255, bitmap.GetPixel(27, 10));
        Assert.Equal(255, bitmap.GetPixel(26, 11));
        Assert.Equal(255, bitmap.GetPixel(25, 12));
    }

    [Fact]
    public void GenerateSliceArchive_UsesRequestedResolutionAndProducesPngLayers()
    {
        var sut = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            new OrthographicProjectionSliceBitmapGenerator(),
        ]);

        var bytes = sut.GenerateSliceArchive(
            CreateTetrahedron(),
            bedWidthMm: 20,
            bedDepthMm: 20,
            resolutionX: 64,
            resolutionY: 32,
            layerHeightMm: 1f);

        using var ms = new MemoryStream(bytes);
        using var archive = new ZipArchive(ms, ZipArchiveMode.Read);

        var manifestEntry = archive.GetEntry("manifest.json");
        Assert.NotNull(manifestEntry);

        using (var manifestStream = manifestEntry!.Open())
        {
            var manifest = JsonDocument.Parse(manifestStream);
            Assert.Equal(64, manifest.RootElement.GetProperty("resolutionX").GetInt32());
            Assert.Equal(32, manifest.RootElement.GetProperty("resolutionY").GetInt32());
            Assert.True(manifest.RootElement.GetProperty("layerCount").GetInt32() >= 1);
            Assert.True(manifest.RootElement.GetProperty("layerBatchSize").GetInt32() >= 1);
        }

        var firstPng = archive.Entries.FirstOrDefault(e => e.FullName.EndsWith(".png", StringComparison.OrdinalIgnoreCase));
        Assert.NotNull(firstPng);

        using var pngStream = firstPng!.Open();
        using var image = Image.Load<L8>(pngStream);
        Assert.Equal(64, image.Width);
        Assert.Equal(32, image.Height);

        var litPixelCount = 0;
        image.ProcessPixelRows(accessor =>
        {
            for (var y = 0; y < image.Height; y++)
            {
                foreach (var pixel in accessor.GetRowSpan(y))
                {
                    if (pixel.PackedValue > 0)
                        litPixelCount++;
                }
            }
        });

        Assert.True(litPixelCount > 0);
    }

    [Fact]
    public void GenerateSliceArchive_ForNonBatchGenerator_RendersMultipleLayersConcurrently()
    {
        var trackingGenerator = new TrackingSlowSliceBitmapGenerator();
        var sut = new PlateSliceRasterService(
        [
            trackingGenerator,
        ]);

        var bytes = sut.GenerateSliceArchive(
            CreateTetrahedron(),
            bedWidthMm: 20,
            bedDepthMm: 20,
            resolutionX: 32,
            resolutionY: 32,
            method: PngSliceExportMethod.MeshIntersection,
            layerHeightMm: 0.25f);

        Assert.NotEmpty(bytes);
        Assert.True(
            trackingGenerator.MaxObservedConcurrency > 1,
            $"Expected concurrent layer rendering for non-batch generators, observed {trackingGenerator.MaxObservedConcurrency}.");
    }

    [Theory]
    [InlineData(PngSliceExportMethod.MeshIntersection)]
    [InlineData(PngSliceExportMethod.OrthographicProjection)]
    public void RenderLayerBitmap_ForMultipleGroups_MatchesUnionOfIndividualRenders(PngSliceExportMethod method)
    {
        var sut = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            new OrthographicProjectionSliceBitmapGenerator(),
        ]);

        var left = CreateTetrahedron();
        var right = CreateTetrahedron()
            .Select(triangle => new Triangle3D(
                new Vec3(triangle.V0.X + 6f, triangle.V0.Y, triangle.V0.Z),
                new Vec3(triangle.V1.X + 6f, triangle.V1.Y, triangle.V1.Z),
                new Vec3(triangle.V2.X + 6f, triangle.V2.Y, triangle.V2.Z),
                triangle.Normal))
            .ToArray();

        var sliceHeightMm = 1.5f;
        var expected = new SliceBitmap(128, 128);
        foreach (var group in new[] { (IReadOnlyList<Triangle3D>)left, right })
        {
            var bitmap = sut.RenderLayerBitmap(group, sliceHeightMm, 20f, 20f, 128, 128, method, 1f);
            AssertBitmapWithinBounds(bitmap, group, 20f, 20f, 128, 128);
            OrInto(expected, bitmap);
        }

        var actual = sut.RenderLayerBitmap(
            new[] { (IReadOnlyList<Triangle3D>)left, right },
            sliceHeightMm,
            20f,
            20f,
            128,
            128,
            method,
            1f);

        Assert.Equal(expected.Pixels, actual.Pixels);
    }

    [Theory]
    [InlineData(PngSliceExportMethod.MeshIntersection)]
    [InlineData(PngSliceExportMethod.OrthographicProjection)]
    public async Task CurrentPlate_Layer00011_WithLocalData_MatchesUnionOfIndividualRenders_AndStaysWithinBounds(PngSliceExportMethod method)
    {
        var dbPath = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "debug", "data", "findamodel.db"));
        if (!File.Exists(dbPath))
            return;

        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseSqlite($"Data Source={dbPath}")
            .Options;

        await using var db = new ModelCacheContext(options);
        var activeList = await db.PrintingLists
            .Include(list => list.Items)
            .Include(list => list.PrinterConfig)
            .FirstOrDefaultAsync(list => list.IsActive);

        Assert.NotNull(activeList);
        Assert.NotEmpty(activeList!.Items);

        var modelsRoot = await db.AppConfigs
            .Select(config => config.ModelsDirectoryPath)
            .FirstOrDefaultAsync();

        Assert.True(!string.IsNullOrWhiteSpace(modelsRoot) && Directory.Exists(modelsRoot), "Expected a configured local models directory for the real-data plate regression.");

        var printer = activeList.PrinterConfig
            ?? await db.PrinterConfigs.OrderByDescending(config => config.IsDefault).FirstOrDefaultAsync();
        Assert.NotNull(printer);

        var modelIds = activeList.Items.Select(item => item.ModelId).Distinct().ToArray();
        var modelRecords = await db.Models
            .Where(model => modelIds.Contains(model.Id))
            .ToDictionaryAsync(model => model.Id);

        var loader = new ModelLoaderService(NullLoggerFactory.Instance);
        var usedPayloadLayout = false;
        var groups = await TryBuildGroupsFromPayloadAsync(modelRecords, modelsRoot!, loader, printer.BedWidthMm, printer.BedDepthMm);
        if (groups.Count > 0)
        {
            usedPayloadLayout = true;
        }
        else
        {
            groups = new List<IReadOnlyList<Triangle3D>>();
            var rotationSeed = 1337;
            var random = new Random(rotationSeed);
            var marginMm = 8f;
            var gapMm = 6f;
            var cursorX = (-printer.BedWidthMm / 2f) + marginMm;
            var cursorZ = (-printer.BedDepthMm / 2f) + marginMm;
            var currentRowDepthMm = 0f;

            foreach (var item in activeList.Items.OrderBy(item => item.ModelId))
            {
                if (!modelRecords.TryGetValue(item.ModelId, out var model))
                    continue;

                var relativeDirectory = model.Directory.Replace('/', Path.DirectorySeparatorChar);
                var filePath = Path.Combine(modelsRoot!, relativeDirectory, model.FileName);
                if (!File.Exists(filePath))
                    continue;

                var geometry = await loader.LoadModelAsync(filePath, model.FileType);
                if (geometry == null)
                    continue;

                for (var copy = 0; copy < Math.Max(1, item.Quantity); copy++)
                {
                    var angleRad = ((float)random.NextDouble() - 0.5f) * MathF.PI * 1.6f;
                    var rotatedBounds = GetRotatedBounds(geometry.Triangles, angleRad);
                    var rotatedWidthMm = rotatedBounds.maxX - rotatedBounds.minX;
                    var rotatedDepthMm = rotatedBounds.maxZ - rotatedBounds.minZ;

                    if (cursorX + rotatedWidthMm > (printer.BedWidthMm / 2f) - marginMm)
                    {
                        cursorX = (-printer.BedWidthMm / 2f) + marginMm;
                        cursorZ += currentRowDepthMm + gapMm;
                        currentRowDepthMm = 0f;
                    }

                    if (cursorZ + rotatedDepthMm > (printer.BedDepthMm / 2f) - marginMm)
                        break;

                    var centerX = cursorX + (rotatedWidthMm * 0.5f) - rotatedBounds.minX;
                    var centerZ = cursorZ + (rotatedDepthMm * 0.5f) - rotatedBounds.minZ;
                    groups.Add(PlaceTriangles(geometry.Triangles, centerX, centerZ, angleRad));

                    cursorX += rotatedWidthMm + gapMm;
                    currentRowDepthMm = MathF.Max(currentRowDepthMm, rotatedDepthMm);
                }
            }
        }

        Assert.NotEmpty(groups);

        var sut = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            new OrthographicProjectionSliceBitmapGenerator(),
        ]);

        const int layerIndex = 11;
        const float layerHeightMm = PlateSliceRasterService.DefaultLayerHeightMm;
        var sliceHeightMm = (layerIndex * layerHeightMm) + (layerHeightMm * 0.5f);

        var expected = new SliceBitmap(printer.PixelWidth, printer.PixelHeight);
        var perObjectBitmaps = new List<SliceBitmap>();
        foreach (var group in groups)
        {
            var bitmap = sut.RenderLayerBitmap(
                group,
                sliceHeightMm,
                printer.BedWidthMm,
                printer.BedDepthMm,
                printer.PixelWidth,
                printer.PixelHeight,
                method,
                layerHeightMm);

            perObjectBitmaps.Add(bitmap);
            AssertBitmapWithinBounds(bitmap, group, printer.BedWidthMm, printer.BedDepthMm, printer.PixelWidth, printer.PixelHeight);
            OrInto(expected, bitmap);
        }

        var actual = sut.RenderLayerBitmap(
            groups,
            sliceHeightMm,
            printer.BedWidthMm,
            printer.BedDepthMm,
            printer.PixelWidth,
            printer.PixelHeight,
            method,
            layerHeightMm);

        ExportDebugBitmaps(usedPayloadLayout ? "current-plate-layer-00011-payload" : "current-plate-layer-00011-rotated", method, expected, actual, perObjectBitmaps);

        Assert.Equal(expected.Pixels, actual.Pixels);
    }

    [Fact]
    public void OrthographicProjectionBatch_MatchesSingleLayerRendering()
    {
        var generator = new OrthographicProjectionSliceBitmapGenerator();
        var batchGenerator = Assert.IsAssignableFrom<IBatchPlateSliceBitmapGenerator>(generator);
        var triangles = CreateTetrahedron();
        var sliceHeights = new[] { 0.5f, 1.5f, 2.5f };
        var perLayerTriangles = sliceHeights
            .Select(_ => triangles)
            .Cast<IReadOnlyList<Triangle3D>>()
            .ToArray();

        var batched = batchGenerator.RenderLayerBitmaps(
            perLayerTriangles,
            sliceHeights,
            bedWidthMm: 20,
            bedDepthMm: 20,
            pixelWidth: 64,
            pixelHeight: 64,
            layerThicknessMm: 1f);

        Assert.Equal(sliceHeights.Length, batched.Count);

        for (var i = 0; i < sliceHeights.Length; i++)
        {
            var single = generator.RenderLayerBitmap(
                triangles,
                sliceHeights[i],
                bedWidthMm: 20,
                bedDepthMm: 20,
                pixelWidth: 64,
                pixelHeight: 64,
                layerThicknessMm: 1f);

            Assert.Equal(single.Pixels, batched[i].Pixels);
        }
    }

    [Fact]
    public void OrthographicProjectionGpu_WhenAvailable_MatchesCpuRendering()
    {
        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        if (!gpuContext.IsAvailable)
            return;

        var triangles = CreateTetrahedron();
        var cpu = new OrthographicProjectionSliceBitmapGenerator();
        var gpu = new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance);

        var expected = cpu.RenderLayerBitmap(triangles, 1.5f, 20f, 20f, 180, 180, 1f);
        var actual = gpu.RenderLayerBitmap(triangles, 1.5f, 20f, 20f, 180, 180, 1f);

        AssertGpuMatchesCpu(expected, actual);
    }

    [Fact]
    public void OrthographicProjectionGpu_WhenAvailable_MatchesCpuRendering_WithOddResolution()
    {
        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        if (!gpuContext.IsAvailable)
            return;

        var triangles = CreateTetrahedron();
        var cpu = new OrthographicProjectionSliceBitmapGenerator();
        var gpu = new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance);

        var expected = cpu.RenderLayerBitmap(triangles, 1.5f, 20f, 20f, 173, 181, 1f);
        var actual = gpu.RenderLayerBitmap(triangles, 1.5f, 20f, 20f, 173, 181, 1f);

        AssertGpuMatchesCpu(expected, actual);
    }

    [Fact]
    public void OrthographicProjectionGpu_WhenAvailable_MatchesCpuRendering_ForDenseMultiObjectScene()
    {
        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        if (!gpuContext.IsAvailable)
            return;

        var cpu = new OrthographicProjectionSliceBitmapGenerator();
        var gpu = new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance);
        var source = CreateTetrahedron();
        var groups = Enumerable.Range(0, 20)
            .Select(index => source
                .Select(triangle => new Triangle3D(
                    new Vec3(triangle.V0.X + (index * 5f), triangle.V0.Y, triangle.V0.Z),
                    new Vec3(triangle.V1.X + (index * 5f), triangle.V1.Y, triangle.V1.Z),
                    new Vec3(triangle.V2.X + (index * 5f), triangle.V2.Y, triangle.V2.Z),
                    triangle.Normal))
                .ToArray())
            .Cast<IReadOnlyList<Triangle3D>>()
            .ToArray();

        var expected = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            cpu,
        ]).RenderLayerBitmap(groups, 1.5f, 120f, 20f, 720, 220, PngSliceExportMethod.OrthographicProjection, 1f);

        var actual = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            gpu,
        ]).RenderLayerBitmap(groups, 1.5f, 120f, 20f, 720, 220, PngSliceExportMethod.OrthographicProjection, 1f);

        AssertGpuMatchesCpu(expected, actual);
    }

    private sealed record PayloadPlacement(string ModelId, int InstanceIndex, double XMm, double YMm, double AngleRad);
    private sealed record PlatePayload(IReadOnlyList<PayloadPlacement> Placements, string? Format, Guid? PrinterConfigId);

    private static async Task<List<IReadOnlyList<Triangle3D>>> TryBuildGroupsFromPayloadAsync(
        IReadOnlyDictionary<Guid, findamodel.Data.Entities.CachedModel> modelRecords,
        string modelsRoot,
        ModelLoaderService loader,
        float bedWidthMm,
        float bedDepthMm)
    {
        var candidatePaths = new[]
        {
            Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "backend.Tests", "TestData", "plate-export-repro-payload.json")),
            Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "artifacts", "plate-slice-debug", "plate-export-repro-payload.json")),
        };

        PlatePayload? payload = null;
        foreach (var path in candidatePaths)
        {
            if (!File.Exists(path))
                continue;

            payload = JsonSerializer.Deserialize<PlatePayload>(await File.ReadAllTextAsync(path), new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true,
            });

            if (payload?.Placements?.Count > 0)
                break;
        }

        if (payload?.Placements?.Count > 0)
        {
            var groups = new List<IReadOnlyList<Triangle3D>>(payload.Placements.Count);
            var geometryCache = new Dictionary<Guid, LoadedGeometry>();

            foreach (var placement in payload.Placements)
            {
                if (!Guid.TryParse(placement.ModelId, out var modelId))
                    continue;
                if (!modelRecords.TryGetValue(modelId, out var model))
                    continue;

                if (!geometryCache.TryGetValue(modelId, out var geometry))
                {
                    var relativeDirectory = model.Directory.Replace('/', Path.DirectorySeparatorChar);
                    var filePath = Path.Combine(modelsRoot, relativeDirectory, model.FileName);
                    if (!File.Exists(filePath))
                        continue;

                    geometry = await loader.LoadModelAsync(filePath, model.FileType) ?? throw new InvalidOperationException($"Failed to load geometry for payload model {model.FileName}.");
                    geometryCache[modelId] = geometry;
                }

                var normalizedX = (float)(placement.XMm - (bedWidthMm * 0.5f));
                var normalizedZ = (float)((bedDepthMm * 0.5f) - placement.YMm);
                groups.Add(PlaceTriangles(geometry.Triangles, normalizedX, normalizedZ, (float)placement.AngleRad));
            }

            return groups;
        }

        return [];
    }

    private static IReadOnlyList<Triangle3D> PlaceTriangles(IReadOnlyList<Triangle3D> triangles, float offsetX, float offsetZ, float angleRad = 0f)
    {
        var sinA = MathF.Sin(angleRad);
        var cosA = MathF.Cos(angleRad);

        static Vec3 Rotate(Vec3 vertex, float sin, float cos)
            => new(vertex.X * cos - vertex.Z * sin, vertex.Y, vertex.X * sin + vertex.Z * cos);

        return triangles
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
            .ToArray();
    }

    private static (float minX, float maxX, float minZ, float maxZ) GetRotatedBounds(IReadOnlyList<Triangle3D> triangles, float angleRad)
    {
        var sinA = MathF.Sin(angleRad);
        var cosA = MathF.Cos(angleRad);
        float minX = float.MaxValue;
        float maxX = float.MinValue;
        float minZ = float.MaxValue;
        float maxZ = float.MinValue;

        void Include(Vec3 vertex)
        {
            var x = vertex.X * cosA - vertex.Z * sinA;
            var z = vertex.X * sinA + vertex.Z * cosA;
            minX = MathF.Min(minX, x);
            maxX = MathF.Max(maxX, x);
            minZ = MathF.Min(minZ, z);
            maxZ = MathF.Max(maxZ, z);
        }

        foreach (var triangle in triangles)
        {
            Include(triangle.V0);
            Include(triangle.V1);
            Include(triangle.V2);
        }

        return (minX, maxX, minZ, maxZ);
    }

    private static void AssertBitmapWithinBounds(
        SliceBitmap bitmap,
        IReadOnlyList<Triangle3D> triangles,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        var minX = triangles.Min(triangle => MathF.Min(triangle.V0.X, MathF.Min(triangle.V1.X, triangle.V2.X)));
        var maxX = triangles.Max(triangle => MathF.Max(triangle.V0.X, MathF.Max(triangle.V1.X, triangle.V2.X)));
        var minZ = triangles.Min(triangle => MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z)));
        var maxZ = triangles.Max(triangle => MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z)));

        var minColumn = Math.Clamp((int)MathF.Floor(((minX + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth) - 1, 0, pixelWidth - 1);
        var maxColumn = Math.Clamp((int)MathF.Ceiling(((maxX + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth) + 1, 0, pixelWidth - 1);
        var minRow = Math.Clamp((int)MathF.Floor((((bedDepthMm * 0.5f) - maxZ) / bedDepthMm) * pixelHeight) - 1, 0, pixelHeight - 1);
        var maxRow = Math.Clamp((int)MathF.Ceiling((((bedDepthMm * 0.5f) - minZ) / bedDepthMm) * pixelHeight) + 1, 0, pixelHeight - 1);

        for (var row = 0; row < pixelHeight; row++)
        {
            for (var column = 0; column < pixelWidth; column++)
            {
                if (bitmap.GetPixel(column, row) == 0)
                    continue;

                var insideBounds = column >= minColumn && column <= maxColumn && row >= minRow && row <= maxRow;
                Assert.True(insideBounds, $"Found lit pixel outside object bounds at ({column}, {row}) expected box x=[{minColumn},{maxColumn}] y=[{minRow},{maxRow}].");
            }
        }
    }

    private static void OrInto(SliceBitmap target, SliceBitmap source)
    {
        for (var i = 0; i < target.Pixels.Length; i++)
        {
            if (source.Pixels[i] > 0)
                target.Pixels[i] = byte.MaxValue;
        }
    }

    private static void ExportDebugBitmaps(
        string scenarioName,
        PngSliceExportMethod method,
        SliceBitmap expected,
        SliceBitmap actual,
        IReadOnlyList<SliceBitmap> perObjectBitmaps)
    {
        var outputDirectory = Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory,
            "..",
            "..",
            "..",
            "..",
            "artifacts",
            "plate-slice-debug",
            scenarioName,
            method.ToString().ToLowerInvariant()));

        Directory.CreateDirectory(outputDirectory);

        SaveBitmap(Path.Combine(outputDirectory, "00_expected_union.png"), expected);
        SaveBitmap(Path.Combine(outputDirectory, "01_actual_full_plate.png"), actual);
        SaveBitmap(Path.Combine(outputDirectory, "02_unexpected_pixels.png"), CreateDifferenceBitmap(actual, expected));
        SaveBitmap(Path.Combine(outputDirectory, "03_missing_pixels.png"), CreateDifferenceBitmap(expected, actual));

        for (var i = 0; i < perObjectBitmaps.Count; i++)
            SaveBitmap(Path.Combine(outputDirectory, $"object_{i + 1:00}.png"), perObjectBitmaps[i]);
    }

    private static SliceBitmap CreateDifferenceBitmap(SliceBitmap source, SliceBitmap subtract)
    {
        var diff = new SliceBitmap(source.Width, source.Height);
        for (var i = 0; i < source.Pixels.Length; i++)
        {
            if (source.Pixels[i] > 0 && subtract.Pixels[i] == 0)
                diff.Pixels[i] = byte.MaxValue;
        }

        return diff;
    }

    private static void SaveBitmap(string path, SliceBitmap bitmap)
    {
        using var image = new Image<L8>(bitmap.Width, bitmap.Height);
        image.ProcessPixelRows(accessor =>
        {
            for (var y = 0; y < bitmap.Height; y++)
            {
                var row = accessor.GetRowSpan(y);
                for (var x = 0; x < bitmap.Width; x++)
                    row[x] = new L8(bitmap.GetPixel(x, y));
            }
        });

        image.SaveAsPng(path);
    }

    private static void AssertGpuMatchesCpu(SliceBitmap expected, SliceBitmap actual)
    {
        var intersection = 0;
        var union = 0;
        for (var i = 0; i < expected.Pixels.Length; i++)
        {
            var expectedOn = expected.Pixels[i] > 0;
            var actualOn = actual.Pixels[i] > 0;
            if (expectedOn && actualOn)
                intersection++;
            if (expectedOn || actualOn)
                union++;
        }

        var iou = union == 0 ? 1f : intersection / (float)union;
        var areaDelta = Math.Abs(expected.CountLitPixels() - actual.CountLitPixels()) / (float)Math.Max(1, expected.CountLitPixels());

        Assert.True(iou >= 0.90f, $"Expected GPU/CPU IoU >= 0.90, actual {iou:F4}.");
        Assert.True(areaDelta <= 0.12f, $"Expected GPU/CPU area delta <= 0.12, actual {areaDelta:F4}.");
    }

    private sealed class TrackingSlowSliceBitmapGenerator : IPlateSliceBitmapGenerator
    {
        private int currentConcurrency;
        private int maxObservedConcurrency;

        public int MaxObservedConcurrency => maxObservedConcurrency;

        public PngSliceExportMethod Method => PngSliceExportMethod.MeshIntersection;

        public SliceBitmap RenderLayerBitmap(
            IReadOnlyList<Triangle3D> triangles,
            float sliceHeightMm,
            float bedWidthMm,
            float bedDepthMm,
            int pixelWidth,
            int pixelHeight,
            float layerThicknessMm = PlateSliceRasterService.DefaultLayerHeightMm)
        {
            var concurrency = System.Threading.Interlocked.Increment(ref currentConcurrency);
            UpdateMaxConcurrency(concurrency);

            try
            {
                System.Threading.Thread.Sleep(40);
                var bitmap = new SliceBitmap(pixelWidth, pixelHeight);
                bitmap.Pixels[0] = byte.MaxValue;
                return bitmap;
            }
            finally
            {
                System.Threading.Interlocked.Decrement(ref currentConcurrency);
            }
        }

        private void UpdateMaxConcurrency(int observed)
        {
            int snapshot;
            do
            {
                snapshot = maxObservedConcurrency;
                if (observed <= snapshot)
                    return;
            }
            while (System.Threading.Interlocked.CompareExchange(ref maxObservedConcurrency, observed, snapshot) != snapshot);
        }
    }
}
