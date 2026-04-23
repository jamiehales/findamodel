using findamodel.Models;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class CtbExportServiceTests
{
    [Fact]
    public void EncodeLayerRle_RoundTripsPixelBuffer()
    {
        var pixels = Enumerable.Range(0, 8192)
            .Select(i => i % 13 == 0 || i % 17 == 0 ? (byte)255 : (byte)0)
            .ToArray();

        var encoded = CtbExportService.EncodeLayerRle(pixels);
        var decoded = CtbExportService.DecodeLayerRle(encoded, pixels.Length);

        Assert.Equal(pixels, decoded);
    }

    [Fact]
    public void GenerateFile_WritesCtbMagicAndPayload()
    {
        var sliceRasterService = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            new OrthographicProjectionSliceBitmapGenerator(),
        ]);
        var sut = new CtbExportService(sliceRasterService);

        List<Triangle3D> group =
        [
            new(
                new Vec3(-10f, 0f, -10f),
                new Vec3(10f, 0f, -10f),
                new Vec3(0f, 2f, 10f),
                Vec3.Up),
        ];

        var printer = new PrinterConfigDto(
            Guid.NewGuid(),
            "Test Printer",
            228f,
            128f,
            7680,
            4320,
            0.05f,
            4,
            0,
            2.5f,
            30f,
            6f,
            65f,
            6f,
            80f,
            150f,
            0f,
            0f,
            0f,
            0f,
            0f,
            255,
            255,
            false,
            true);

        var bytes = sut.GenerateFile([group], printer, progressReporter: null, CancellationToken.None);

        Assert.True(bytes.Length > 512, "Expected a non-trivial CTB payload.");
        Assert.Equal((byte)0x07, bytes[0]);
        Assert.Equal((byte)0x01, bytes[1]);
        Assert.Equal((byte)0xFD, bytes[2]);
        Assert.Equal((byte)0x12, bytes[3]);
    }
}
