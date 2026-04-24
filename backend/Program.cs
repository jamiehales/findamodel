using System.IO.Compression;
using System.Diagnostics;
using findamodel.Auth;
using findamodel.Data;
using findamodel.Services;
using Microsoft.Data.Sqlite;
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
builder.Services.AddSingleton<findamodel.Services.AutoSupportGenerationService>();
builder.Services.AddSingleton<findamodel.Services.AutoSupportJobService>();
builder.Services.AddSingleton<findamodel.Services.AutoSupportSettingsPreviewService>();
builder.Services.AddSingleton<findamodel.Services.ModelSaverService>();
builder.Services.AddSingleton<findamodel.Services.GlSliceProjectionContext>();
builder.Services.AddSingleton<findamodel.Services.IPlateSliceBitmapGenerator, findamodel.Services.MeshIntersectionSliceBitmapGenerator>();
builder.Services.AddSingleton<findamodel.Services.IPlateSliceBitmapGenerator, findamodel.Services.OrthographicProjectionSliceBitmapGenerator>();
builder.Services.AddSingleton<findamodel.Services.PlateSliceRasterService>();
builder.Services.AddSingleton<findamodel.Services.CtbExportService>();
builder.Services.AddSingleton<findamodel.Services.ModelRepairService>();
builder.Services.AddSingleton<findamodel.Services.PlateModelGeometryCacheService>();
builder.Services.AddSingleton<findamodel.Services.PlateExportService>();
builder.Services.AddSingleton<findamodel.Services.PlateGenerationJobService>();
builder.Services.AddSingleton<findamodel.Services.PlateSlicePreviewService>();
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

    Log.Information("Startup DB preflight beginning for {DbPath}", dbPath);
    EnsureDatabaseUnlockedOrFailFast(dbPath);
    Log.Information("Startup DB preflight complete for {DbPath}", dbPath);

    var dbFactory = scope.ServiceProvider.GetRequiredService<IDbContextFactory<ModelCacheContext>>();
    Log.Information("Creating ModelCacheContext for {DbPath}", dbPath);
    using var db = dbFactory.CreateDbContext();
    db.Database.SetCommandTimeout(TimeSpan.FromSeconds(60));

    try
    {
        // If the database was created when the V3-named migration existed, it will have
        // '20260419193444_AddAutoSupportV3ForceSettings' in __EFMigrationsHistory but not
        // '20260419193444_AddAutoSupportForceSettings'. EF would then try to run the
        // non-V3 migration and fail with "duplicate column name". Detect and handle this.
        try
        {
            await db.Database.ExecuteSqlRawAsync(@"
                INSERT OR IGNORE INTO __EFMigrationsHistory (MigrationId, ProductVersion)
                SELECT '20260419193444_AddAutoSupportForceSettings', '10.0.6'
                WHERE EXISTS (
                    SELECT 1 FROM __EFMigrationsHistory
                    WHERE MigrationId = '20260419193444_AddAutoSupportV3ForceSettings'
                )
                AND NOT EXISTS (
                    SELECT 1 FROM __EFMigrationsHistory
                    WHERE MigrationId = '20260419193444_AddAutoSupportForceSettings'
                )");
        }
        catch (Exception ex)
        {
            Log.Warning(ex, "V3 migration alias check skipped (table may not exist yet)");
        }

        Log.Information("Starting database migration for {DbPath}", dbPath);
        using var migrateCts = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        await db.Database.MigrateAsync(migrateCts.Token);
        Log.Information("Database migration complete for {DbPath}", dbPath);

        Log.Information("Applying SQLite pragmas for {DbPath}", dbPath);
        db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;");
        db.Database.ExecuteSqlRaw("PRAGMA busy_timeout=10000;");
        Log.Information("SQLite pragmas applied for {DbPath}", dbPath);

        var setupConfig = db.AppConfigs.AsNoTracking().FirstOrDefault(c => c.Id == 1);
        if (setupConfig?.SetupCompleted == true && !string.IsNullOrWhiteSpace(setupConfig.ModelsDirectoryPath))
            app.Configuration["Models:DirectoryPath"] = setupConfig.ModelsDirectoryPath;
    }
    catch (OperationCanceledException ex)
    {
        var message =
            $"Database migration timed out after 30 seconds for {dbPath}. " +
            "This is usually caused by a locked or busy SQLite database.";
        Log.Fatal(ex, message);
        throw new InvalidOperationException(message, ex);
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

static void EnsureDatabaseUnlockedOrFailFast(string dbPath)
{
    if (!IsSqliteDatabaseLocked(dbPath, out var initialLockError))
        return;

    var holderPids = GetPotentialLockHolderPids(dbPath);
    var killedPids = new List<int>();
    var currentPid = Environment.ProcessId;

    foreach (var pid in holderPids.Where(pid => pid != currentPid))
    {
        if (TryKillProcess(pid, out var killError))
        {
            killedPids.Add(pid);
            Log.Warning("Killed process {Pid} while clearing SQLite lock on {DbPath}", pid, dbPath);
        }
        else if (!string.IsNullOrWhiteSpace(killError))
        {
            Log.Warning("Failed to kill process {Pid} while clearing SQLite lock on {DbPath}: {Error}", pid, dbPath, killError);
        }
    }

    if (killedPids.Count > 0)
        Thread.Sleep(400);

    if (!IsSqliteDatabaseLocked(dbPath, out var retryLockError))
    {
        Log.Warning("SQLite lock cleared for {DbPath}. Startup continuing.", dbPath);
        return;
    }

    var pidText = holderPids.Count > 0
        ? string.Join(", ", holderPids)
        : "unknown";

    var message =
        $"Database file is locked and startup cannot continue. " +
        $"DbPath: {dbPath}. " +
        $"Lock holders: {pidText}. " +
        $"Lock error: {retryLockError ?? initialLockError ?? "unknown"}";

    Log.Fatal(message);
    throw new InvalidOperationException(message);
}

static bool IsSqliteDatabaseLocked(string dbPath, out string? lockError)
{
    lockError = null;

    try
    {
        using var connection = new SqliteConnection($"Data Source={dbPath};Mode=ReadWriteCreate;Pooling=False");
        connection.Open();

        using var command = connection.CreateCommand();
        command.CommandText = "PRAGMA busy_timeout=250; BEGIN IMMEDIATE; ROLLBACK;";
        command.ExecuteNonQuery();
        return false;
    }
    catch (SqliteException ex) when (ex.SqliteErrorCode is 5 or 6)
    {
        lockError = ex.Message;
        return true;
    }
}

static List<int> GetPotentialLockHolderPids(string dbPath)
{
    try
    {
        var candidates = new[] { dbPath, $"{dbPath}-wal", $"{dbPath}-shm" }
            .Where(File.Exists)
            .ToList();

        if (candidates.Count == 0)
            return [];

        var lsofPath = File.Exists("/usr/sbin/lsof") ? "/usr/sbin/lsof" : "lsof";
        var startInfo = new ProcessStartInfo
        {
            FileName = lsofPath,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };

        startInfo.ArgumentList.Add("-t");
        foreach (var path in candidates)
            startInfo.ArgumentList.Add(path);

        using var process = Process.Start(startInfo);
        if (process == null)
            return [];

        var output = process.StandardOutput.ReadToEnd();
        process.WaitForExit(2000);

        return output
            .Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries)
            .Select(line => int.TryParse(line.Trim(), out var pid) ? pid : (int?)null)
            .Where(pid => pid.HasValue)
            .Select(pid => pid!.Value)
            .Distinct()
            .ToList();
    }
    catch
    {
        return [];
    }
}

static bool TryKillProcess(int pid, out string? error)
{
    error = null;

    try
    {
        var process = Process.GetProcessById(pid);
        process.Kill(entireProcessTree: true);
        process.WaitForExit(3000);
        return true;
    }
    catch (Exception ex)
    {
        error = ex.Message;
        return false;
    }
}
