using System.IO.Compression;
using findamodel.Auth;
using findamodel.Data;
using findamodel.Services;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.ResponseCompression;
using Microsoft.EntityFrameworkCore;
using Serilog;

var builder = WebApplication.CreateBuilder(args);
var applicationLogBuffer = new ApplicationLogBuffer();

var desktopMode = string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_MODE"), "desktop", StringComparison.OrdinalIgnoreCase);
var desktopUrl = Environment.GetEnvironmentVariable("FINDAMODEL_URL");
var desktopToken = Environment.GetEnvironmentVariable("FINDAMODEL_DESKTOP_SESSION_TOKEN");
var disableCors = bool.TryParse(Environment.GetEnvironmentVariable("FINDAMODEL_DISABLE_CORS"), out var disableCorsValue)
    && disableCorsValue;

if (desktopMode)
{
    if (string.IsNullOrWhiteSpace(desktopUrl))
        throw new InvalidOperationException("FINDAMODEL_URL is required in desktop mode.");

    if (!Uri.TryCreate(desktopUrl, UriKind.Absolute, out var desktopUri))
        throw new InvalidOperationException("FINDAMODEL_URL must be a valid absolute URL in desktop mode.");

    var localhostHost = string.Equals(desktopUri.Host, "localhost", StringComparison.OrdinalIgnoreCase)
        || string.Equals(desktopUri.Host, "127.0.0.1", StringComparison.OrdinalIgnoreCase)
        || string.Equals(desktopUri.Host, "::1", StringComparison.OrdinalIgnoreCase);

    if (!localhostHost)
        throw new InvalidOperationException("FINDAMODEL_URL must bind to localhost in desktop mode.");

    if (string.IsNullOrWhiteSpace(desktopToken))
        throw new InvalidOperationException("FINDAMODEL_DESKTOP_SESSION_TOKEN is required in desktop mode.");

    builder.WebHost.UseUrls(desktopUrl);
}

builder.Host.UseSerilog((ctx, cfg) =>
{
    cfg.ReadFrom.Configuration(ctx.Configuration)
    .Enrich.FromLogContext()
    .WriteTo.Sink(new ApplicationLogSink(applicationLogBuffer));

    // Seq integration: set Seq:ServerUrl in appsettings.Development.json (or user secrets)
    // to stream structured logs to Seq for dynamic channel filtering.
    // Example: "Seq": { "ServerUrl": "http://localhost:5341" }
    var seqUrl = ctx.Configuration["Seq:ServerUrl"];
    if (!string.IsNullOrEmpty(seqUrl))
        cfg.WriteTo.Seq(seqUrl);
});

builder.Services.AddControllers();
builder.Services.AddSingleton(applicationLogBuffer);

var dataPath = Environment.GetEnvironmentVariable("FINDAMODEL_DATA_PATH")
    ?? builder.Configuration["Configuration:DataPath"]
    ?? "data";
var resolvedDataPath = Path.GetFullPath(dataPath);
Directory.CreateDirectory(resolvedDataPath);

var dbPath = Path.Combine(resolvedDataPath, "findamodel.db");
builder.Services.AddDbContextFactory<ModelCacheContext>(options =>
    options.UseSqlite($"Data Source={dbPath}"));

var cacheRendersPath = Path.Combine(resolvedDataPath, "cache", "renders");
Directory.CreateDirectory(cacheRendersPath);
builder.Configuration["Cache:RendersPath"] = cacheRendersPath;

var cacheAutoSupportsPath = Path.Combine(resolvedDataPath, "cache", "auto-support");
Directory.CreateDirectory(cacheAutoSupportsPath);
builder.Configuration["Cache:AutoSupportsPath"] = cacheAutoSupportsPath;

builder.Services.AddSingleton<findamodel.Services.UserService>();
builder.Services.AddAuthentication("AutoAdmin")
    .AddScheme<AuthenticationSchemeOptions, AutoAdminAuthHandler>("AutoAdmin", null);

builder.Services.AddSingleton<findamodel.Services.DirectoryConfigReader>();
builder.Services.AddSingleton<findamodel.Services.ModelLoaderService>();
builder.Services.AddSingleton<findamodel.Services.MeshTransferService>();
builder.Services.AddSingleton<findamodel.Services.SupportSeparationService>();
builder.Services.AddSingleton<findamodel.Services.AutoSupportGenerationV3Service>();
builder.Services.AddSingleton<findamodel.Services.AutoSupportJobService>();
builder.Services.AddSingleton<findamodel.Services.ModelSaverService>();
builder.Services.AddSingleton<findamodel.Services.GlSliceProjectionContext>();
builder.Services.AddSingleton<findamodel.Services.IPlateSliceBitmapGenerator, findamodel.Services.MeshIntersectionSliceBitmapGenerator>();
builder.Services.AddSingleton<findamodel.Services.IPlateSliceBitmapGenerator, findamodel.Services.OrthographicProjectionSliceBitmapGenerator>();
builder.Services.AddSingleton<findamodel.Services.PlateSliceRasterService>();
builder.Services.AddSingleton<findamodel.Services.PlateExportService>();
builder.Services.AddSingleton<findamodel.Services.PlateGenerationJobService>();
builder.Services.AddSingleton<findamodel.Services.GlPreviewContext>();
builder.Services.AddSingleton<findamodel.Services.ModelPreviewService>();
builder.Services.AddSingleton<findamodel.Services.HullCalculationService>();
builder.Services.AddSingleton<findamodel.Services.IPreviewRuntimeInfoProvider, findamodel.Services.PreviewRuntimeInfoProvider>();
builder.Services.AddSingleton<findamodel.Services.MetadataConfigService>();
builder.Services.AddSingleton<findamodel.Services.MetadataDictionaryService>();
builder.Services.AddSingleton<findamodel.Services.AppConfigService>();
builder.Services.AddSingleton<findamodel.Services.InstanceStatsService>();
builder.Services.AddHttpClient(nameof(findamodel.Services.OllamaLocalLlmProvider));
builder.Services.AddHttpClient(nameof(findamodel.Services.InternalLlmModelStore));
builder.Services.AddSingleton<findamodel.Services.InternalLlmModelStore>();
builder.Services.AddSingleton<findamodel.Services.ILocalLlmProvider, findamodel.Services.InternalLocalLlmProvider>();
builder.Services.AddSingleton<findamodel.Services.ILocalLlmProvider, findamodel.Services.OllamaLocalLlmProvider>();
builder.Services.AddSingleton<findamodel.Services.LocalLlmProviderResolver>();
builder.Services.AddSingleton<findamodel.Services.TagGenerationService>();
builder.Services.AddSingleton<findamodel.Services.ModelService>();
builder.Services.AddSingleton<findamodel.Services.IndexerService>();
builder.Services.AddSingleton<findamodel.Services.ExplorerService>();
builder.Services.AddSingleton<findamodel.Services.PrintingListService>();
builder.Services.AddSingleton<findamodel.Services.PrintingListArchiveService>();
builder.Services.AddSingleton<findamodel.Services.PrinterService>();
builder.Services.AddSingleton<findamodel.Services.QueryService>();
builder.Services.AddHostedService<findamodel.Services.ModelIndexerService>();
builder.Services.AddHostedService<findamodel.Services.InternalLlmWarmupService>();
builder.Services.AddHostedService<findamodel.Services.LlmStartupDiagnosticsService>();

if (!desktopMode && !disableCors && builder.Environment.IsDevelopment())
{
    builder.Services.AddCors(options =>
    {
        options.AddDefaultPolicy(policy =>
        {
            policy.WithOrigins("http://localhost:5173")
                  .AllowAnyHeader()
                  .AllowAnyMethod();
        });
    });
}

builder.Services.Configure<BrotliCompressionProviderOptions>(options =>
{
    options.Level = CompressionLevel.Optimal;
});

builder.Services.Configure<GzipCompressionProviderOptions>(options =>
{
    options.Level = CompressionLevel.Optimal;
});

builder.Services.AddResponseCompression(options =>
{
    options.Providers.Add<GzipCompressionProvider>();
    options.Providers.Add<BrotliCompressionProvider>();
    options.EnableForHttps = true;
    options.MimeTypes = ["application/json", "application/3mf", "model/gltf-binary", findamodel.Services.MeshTransferService.ContentType];
});

var app = builder.Build();

app.UseResponseCompression();

if (desktopMode)
{
    app.Use(async (context, next) =>
    {
        if (context.Request.Path.StartsWithSegments("/api", StringComparison.OrdinalIgnoreCase))
        {
            var requestToken = context.Request.Headers["X-Findamodel-Desktop-Token"].ToString();
            if (string.IsNullOrWhiteSpace(requestToken))
                requestToken = context.Request.Query["desktopToken"].ToString();

            if (!string.Equals(requestToken, desktopToken, StringComparison.Ordinal))
            {
                context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                await context.Response.WriteAsync("Missing or invalid desktop session token.");
                return;
            }
        }

        await next();
    });
}

using (var scope = app.Services.CreateScope())
{
    var previewService = scope.ServiceProvider.GetRequiredService<findamodel.Services.ModelPreviewService>();
    previewService.SetCacheDirectory(cacheRendersPath);

    var dbFactory = scope.ServiceProvider.GetRequiredService<IDbContextFactory<ModelCacheContext>>();
    using var db = dbFactory.CreateDbContext();
    db.Database.SetCommandTimeout(TimeSpan.FromSeconds(60));

    try
    {
        db.Database.Migrate();
        db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;");
        db.Database.ExecuteSqlRaw("PRAGMA busy_timeout=10000;");

        var setupConfig = db.AppConfigs.AsNoTracking().FirstOrDefault(c => c.Id == 1);
        if (setupConfig?.SetupCompleted == true && !string.IsNullOrWhiteSpace(setupConfig.ModelsDirectoryPath))
            app.Configuration["Models:DirectoryPath"] = setupConfig.ModelsDirectoryPath;
    }
    catch (Exception ex)
    {
        Log.Fatal(ex, "Database startup migration failed for path {DbPath}", dbPath);
        throw;
    }

    var userService = scope.ServiceProvider.GetRequiredService<findamodel.Services.UserService>();
    await userService.SeedAdminUserAsync();

    var indexerService = scope.ServiceProvider.GetRequiredService<findamodel.Services.IndexerService>();
    await indexerService.RequeueInterruptedRunsAsync();

    var printingListService = scope.ServiceProvider.GetRequiredService<findamodel.Services.PrintingListService>();
    var adminUser = await userService.GetAdminUserAsync();
    if (adminUser != null)
        await printingListService.EnsureDefaultListAsync(adminUser.Id);

}

if (!desktopMode && !disableCors && app.Environment.IsDevelopment())
{
    app.UseCors();
}

app.UseDefaultFiles();
app.UseStaticFiles();

app.UseAuthentication();
app.UseAuthorization();

app.MapGet("/health", async (findamodel.Services.LocalLlmProviderResolver llmResolver, findamodel.Services.AppConfigService appConfigService) =>
{
    try
    {
        var config = await appConfigService.GetAsync();
        if (!findamodel.Services.AppConfigService.IsAnyAiGenerationEnabled(config))
            return Results.Ok(new { status = "ok" });

        var provider = config?.TagGenerationProvider ?? "internal";
        var llmProvider = llmResolver.Resolve(provider);

        if (llmProvider != null)
        {
            var settings = new findamodel.Services.LocalLlmProviderSettings(
                Endpoint: config?.TagGenerationEndpoint ?? "http://localhost:11434",
                Model: config?.TagGenerationModel ?? findamodel.Services.AppConfigService.GetDefaultTagGenerationModel(),
                TimeoutMs: config?.TagGenerationTimeoutMs ?? 30000);

            var health = await llmProvider.GetHealthAsync(settings, CancellationToken.None);
            return Results.Ok(new
            {
                status = "ok",
                llm = new
                {
                    reachable = health.Reachable,
                    provider = health.Provider,
                    model = health.Model,
                    backend = health.Metadata?.GetValueOrDefault("backend"),
                    error = health.Error
                }
            });
        }
    }
    catch { }

    return Results.Ok(new { status = "ok" });
});
app.MapControllers();

app.MapFallbackToFile("index.html");

app.Run();
