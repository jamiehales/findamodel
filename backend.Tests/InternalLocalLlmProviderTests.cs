using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class InternalLocalLlmProviderTests
{
    [Fact]
    public void CreateSamplingPipeline_UsesHardcodedSamplingSettings()
    {
        var pipeline = InternalLocalLlmProvider.CreateSamplingPipeline();

        Assert.Equal(0.2f, pipeline.Temperature, 3);
        Assert.Equal(0.9f, pipeline.TopP, 3);
        Assert.Equal(50, pipeline.TopK);
        Assert.Equal(1.05f, pipeline.RepeatPenalty, 3);
    }

    [Theory]
    [InlineData(1, 64)]
    [InlineData(64, 64)]
    [InlineData(256, 256)]
    [InlineData(2048, 2048)]
    [InlineData(4096, 2048)]
    public void ResolveCompletionTokenLimit_ClampsToSupportedRange(int requested, int expected)
    {
        var actual = InternalLocalLlmProvider.ResolveCompletionTokenLimit(requested);
        Assert.Equal(expected, actual);
    }
}
