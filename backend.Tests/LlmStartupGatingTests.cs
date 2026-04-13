using System.Net;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging;
using Xunit;

namespace findamodel.Tests;

public class LlmStartupGatingTests
{
    private static IDbContextFactory<ModelCacheContext> CreateFactory(string dbName)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;
        return new InMemoryDbContextFactory(options);
    }

    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);

        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
            => Task.FromResult(CreateDbContext());
    }

    private static IConfiguration CreateConfiguration(Dictionary<string, string?>? values = null)
    {
        return new ConfigurationBuilder()
            .AddInMemoryCollection(values)
            .Build();
    }

    [Fact]
    public async Task InternalLlmWarmupService_DoesNotInitializeModel_WhenAiGenerationDisabled()
    {
        var dbName = nameof(InternalLlmWarmupService_DoesNotInitializeModel_WhenAiGenerationDisabled);
        var factory = CreateFactory(dbName);
        await using (var db = factory.CreateDbContext())
        {
            db.AppConfigs.Add(new AppConfig
            {
                Id = 1,
                TagGenerationEnabled = false,
                AiDescriptionEnabled = false,
                TagGenerationProvider = "internal",
                TagGenerationModel = AppConfigService.DefaultTagGenerationModel,
            });
            await db.SaveChangesAsync();
        }

        var cachePath = Path.Combine(Path.GetTempPath(), $"findamodel-llm-test-{Guid.NewGuid():N}");
        var config = CreateConfiguration(new Dictionary<string, string?>
        {
            ["LocalLlm:Internal:CachePath"] = cachePath,
        });

        var appConfigService = new AppConfigService(factory, config);
        var httpClientFactory = new TrackingHttpClientFactory();
        var modelStore = new InternalLlmModelStore(config, httpClientFactory, LoggerFactory.Create(_ => { }));
        var warmupService = new InternalLlmWarmupService(modelStore, appConfigService, LoggerFactory.Create(_ => { }));

        await warmupService.StartAsync(CancellationToken.None);

        Assert.False(httpClientFactory.WasCalled);
        Assert.False(Directory.Exists(cachePath));
    }

    [Fact]
    public async Task LlmStartupDiagnosticsService_DoesNotProbeProvider_WhenAiGenerationDisabled()
    {
        var dbName = nameof(LlmStartupDiagnosticsService_DoesNotProbeProvider_WhenAiGenerationDisabled);
        var factory = CreateFactory(dbName);
        await using (var db = factory.CreateDbContext())
        {
            db.AppConfigs.Add(new AppConfig
            {
                Id = 1,
                TagGenerationEnabled = false,
                AiDescriptionEnabled = false,
                TagGenerationProvider = "internal",
                TagGenerationModel = AppConfigService.DefaultTagGenerationModel,
            });
            await db.SaveChangesAsync();
        }

        var appConfigService = new AppConfigService(factory, CreateConfiguration());
        var provider = new TrackingLlmProvider("internal");
        var resolver = new LocalLlmProviderResolver([provider]);
        var service = new TestableLlmStartupDiagnosticsService(appConfigService, resolver, LoggerFactory.Create(_ => { }));

        await service.RunOnce(CancellationToken.None);

        Assert.Equal(0, provider.HealthCalls);
    }

    [Fact]
    public async Task LlmStartupDiagnosticsService_ProbesProvider_WhenAiGenerationEnabled()
    {
        var dbName = nameof(LlmStartupDiagnosticsService_ProbesProvider_WhenAiGenerationEnabled);
        var factory = CreateFactory(dbName);
        await using (var db = factory.CreateDbContext())
        {
            db.AppConfigs.Add(new AppConfig
            {
                Id = 1,
                TagGenerationEnabled = true,
                AiDescriptionEnabled = false,
                TagGenerationProvider = "internal",
                TagGenerationModel = AppConfigService.DefaultTagGenerationModel,
            });
            await db.SaveChangesAsync();
        }

        var appConfigService = new AppConfigService(factory, CreateConfiguration());
        var provider = new TrackingLlmProvider("internal");
        var resolver = new LocalLlmProviderResolver([provider]);
        var service = new TestableLlmStartupDiagnosticsService(appConfigService, resolver, LoggerFactory.Create(_ => { }));

        await service.RunOnce(CancellationToken.None);

        Assert.Equal(1, provider.HealthCalls);
    }

    private sealed class TestableLlmStartupDiagnosticsService(
        AppConfigService appConfigService,
        LocalLlmProviderResolver providerResolver,
        ILoggerFactory loggerFactory)
        : LlmStartupDiagnosticsService(appConfigService, providerResolver, loggerFactory)
    {
        public Task RunOnce(CancellationToken ct) => ExecuteAsync(ct);
    }

    private sealed class TrackingLlmProvider(string name) : ILocalLlmProvider
    {
        public string Name { get; } = name;
        public int HealthCalls { get; private set; }

        public Task<LocalLlmHealth> GetHealthAsync(LocalLlmProviderSettings settings, CancellationToken ct)
        {
            HealthCalls++;
            return Task.FromResult(new LocalLlmHealth(
                Reachable: true,
                ModelReady: true,
                Provider: Name,
                Model: settings.Model,
                Error: null,
                Metadata: null));
        }

        public Task<LocalLlmResponse> GenerateAsync(LocalLlmProviderSettings settings, LocalLlmRequest request, CancellationToken ct)
        {
            return Task.FromResult(LocalLlmResponse.Empty(Name, settings.Model, TimeSpan.Zero));
        }
    }

    private sealed class TrackingHttpClientFactory : IHttpClientFactory
    {
        public bool WasCalled { get; private set; }

        public HttpClient CreateClient(string name)
        {
            WasCalled = true;
            return new HttpClient(new ThrowingHttpHandler());
        }
    }

    private sealed class ThrowingHttpHandler : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.InternalServerError));
        }
    }
}