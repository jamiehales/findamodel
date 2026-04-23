using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class PlateExportServiceResolutionScaleTests
{
    [Theory]
    [InlineData(null, 0.25f)]
    [InlineData(0.125f, 0.125f)]
    [InlineData(0.25f, 0.25f)]
    [InlineData(0.5f, 0.5f)]
    [InlineData(1f, 1f)]
    public void NormalizePreviewResolutionScale_AllowsExpectedValues(float? input, float expected)
    {
        var actual = PlateExportService.NormalizePreviewResolutionScale(input);

        Assert.Equal(expected, actual);
    }

    [Theory]
    [InlineData(0f)]
    [InlineData(0.2f)]
    [InlineData(0.75f)]
    [InlineData(1.5f)]
    public void NormalizePreviewResolutionScale_RejectsUnexpectedValues(float input)
    {
        Assert.Throws<ArgumentException>(() => PlateExportService.NormalizePreviewResolutionScale(input));
    }
}
