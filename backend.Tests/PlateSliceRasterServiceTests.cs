using System.IO.Compression;
using System.Text.Json;
using findamodel.Services;
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
}
