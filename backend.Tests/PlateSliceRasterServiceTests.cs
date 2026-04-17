using System.IO.Compression;
using System.Text.Json;
using findamodel.Services;
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
}
