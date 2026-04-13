using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class OllamaLocalLlmProviderTests
{
    [Fact]
    public async Task GenerateAsync_SendsHardcodedDecodingOptions()
    {
        var handler = new CapturingHandler(_ =>
            new HttpResponseMessage(HttpStatusCode.OK)
            {
                Content = new StringContent("""{"response":"{\"description\":\"an orc\",\"confidence\":0.9}"}""", Encoding.UTF8, "application/json"),
            });

        var sut = new OllamaLocalLlmProvider(new StubHttpClientFactory(handler), NullLoggerFactory.Instance);
        var settings = new LocalLlmProviderSettings("http://localhost:11434", "qwen2.5vl:7b", 30000);
        var request = new LocalLlmRequest
        {
            TaskKind = LocalLlmTaskKind.Description,
            SystemPrompt = "sys",
            UserPrompt = "usr",
            MaxOutputTokens = 256,
        };

        _ = await sut.GenerateAsync(settings, request, CancellationToken.None);

        Assert.NotNull(handler.LastRequestBody);
        using var doc = JsonDocument.Parse(handler.LastRequestBody!);
        var options = doc.RootElement.GetProperty("options");

        Assert.Equal(0.2, options.GetProperty("temperature").GetDouble(), 3);
        Assert.Equal(0.9, options.GetProperty("top_p").GetDouble(), 3);
        Assert.Equal(50, options.GetProperty("top_k").GetInt32());
        Assert.Equal(1.05, options.GetProperty("repeat_penalty").GetDouble(), 3);
        Assert.Equal(256, options.GetProperty("num_predict").GetInt32());
    }

    [Fact]
    public async Task GenerateAsync_ClampsNumPredictToMinimum64()
    {
        var handler = new CapturingHandler(_ =>
            new HttpResponseMessage(HttpStatusCode.OK)
            {
                Content = new StringContent("""{"response":"{\"tags\":[\"orc\"]}"}""", Encoding.UTF8, "application/json"),
            });

        var sut = new OllamaLocalLlmProvider(new StubHttpClientFactory(handler), NullLoggerFactory.Instance);
        var settings = new LocalLlmProviderSettings("http://localhost:11434", "qwen2.5vl:7b", 30000);
        var request = new LocalLlmRequest
        {
            TaskKind = LocalLlmTaskKind.Tags,
            SystemPrompt = "sys",
            UserPrompt = "usr",
            MaxOutputTokens = 1,
        };

        _ = await sut.GenerateAsync(settings, request, CancellationToken.None);

        Assert.NotNull(handler.LastRequestBody);
        using var doc = JsonDocument.Parse(handler.LastRequestBody!);
        var options = doc.RootElement.GetProperty("options");
        Assert.Equal(64, options.GetProperty("num_predict").GetInt32());
    }

    private sealed class StubHttpClientFactory(HttpMessageHandler handler) : IHttpClientFactory
    {
        public HttpClient CreateClient(string name) => new(handler)
        {
            BaseAddress = new Uri("http://localhost:11434"),
        };
    }

    private sealed class CapturingHandler(Func<HttpRequestMessage, HttpResponseMessage> responseFactory) : HttpMessageHandler
    {
        public string? LastRequestBody { get; private set; }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            LastRequestBody = request.Content == null
                ? null
                : await request.Content.ReadAsStringAsync(cancellationToken);

            return responseFactory(request);
        }
    }
}
