using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ScanConfigTests
{
    [Fact]
    public void Compute_ContainsCurrentHullGenerationVersion()
    {
        var result = ScanConfig.Compute(2f);
        Assert.Contains($"hull:{HullCalculationService.CurrentHullGenerationVersion}", result);
    }

    [Fact]
    public void Compute_ContainsRaftHeightWithFourDecimalPlaces()
    {
        var result = ScanConfig.Compute(1.5f);
        Assert.Contains("raft:1.5000", result);
    }

    [Fact]
    public void Compute_ProducesExpectedFormatString()
    {
        var version = HullCalculationService.CurrentHullGenerationVersion;
        var expected = $"hull:{version}|raft:2.0000";
        Assert.Equal(expected, ScanConfig.Compute(2f));
    }

    [Fact]
    public void Compute_DifferentRaftHeights_ProduceDifferentChecksums()
    {
        Assert.NotEqual(ScanConfig.Compute(1f), ScanConfig.Compute(2f));
    }

    [Fact]
    public void Compute_SameInputs_ProduceSameChecksum()
    {
        Assert.Equal(ScanConfig.Compute(3f), ScanConfig.Compute(3f));
    }

    [Fact]
    public void Compute_ZeroRaftHeight_IsValid()
    {
        var result = ScanConfig.Compute(0f);
        Assert.Contains("raft:0.0000", result);
    }

    [Theory]
    [InlineData(0f, "0.0000")]
    [InlineData(1f, "1.0000")]
    [InlineData(1.5f, "1.5000")]
    [InlineData(2.25f, "2.2500")]
    public void Compute_FormatsRaftHeightToFourDecimalPlaces(float raft, string expectedFragment)
    {
        Assert.Contains(expectedFragment, ScanConfig.Compute(raft));
    }
}
