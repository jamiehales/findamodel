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

    [Fact]
    public void BuildPrompt_AppendsJsonFormatInstructions_ForTags()
    {
        var prompt = InternalLocalLlmProvider.BuildPrompt(new LocalLlmRequest
        {
            TaskKind = LocalLlmTaskKind.Tags,
            SystemPrompt = "system",
            UserPrompt = "user prompt",
            AllowedTags = ["beast", "dragon"],
        });

        Assert.Contains("Respond with JSON exactly", prompt);
        Assert.Contains("Return only the JSON object", prompt);
        Assert.Contains("user prompt", prompt);
    }

    [Fact]
    public void BuildPrompt_DoesNotAppendJsonFormatInstructions_ForDescriptions()
    {
        var prompt = InternalLocalLlmProvider.BuildPrompt(new LocalLlmRequest
        {
            TaskKind = LocalLlmTaskKind.Description,
            SystemPrompt = "system",
            UserPrompt = "user prompt",
        });

        Assert.DoesNotContain("Respond with JSON exactly", prompt);
        Assert.DoesNotContain("Return only the JSON object", prompt);
        Assert.Contains("user prompt", prompt);
    }
}
