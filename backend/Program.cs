using findamodel.Auth;
using findamodel.Data;
using Microsoft.AspNetCore.Authentication;
using Microsoft.EntityFrameworkCore;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddControllers();

var dataPath = builder.Configuration["Configuration:DataPath"] ?? "data";
var resolvedDataPath = Path.GetFullPath(dataPath);
Directory.CreateDirectory(resolvedDataPath);

var dbPath = Path.Combine(resolvedDataPath, "findamodel.db");
builder.Services.AddDbContextFactory<ModelCacheContext>(options =>
    options.UseSqlite($"Data Source={dbPath}"));

var cacheRendersPath = Path.Combine(resolvedDataPath, "cache", "renders");
Directory.CreateDirectory(cacheRendersPath);
builder.Configuration["Cache:RendersPath"] = cacheRendersPath;

builder.Services.AddSingleton<findamodel.Services.UserService>();
builder.Services.AddAuthentication("AutoAdmin")
    .AddScheme<AuthenticationSchemeOptions, AutoAdminAuthHandler>("AutoAdmin", null);

builder.Services.AddSingleton<findamodel.Services.ModelLoaderService>();
builder.Services.AddSingleton<findamodel.Services.ModelSaverService>();
builder.Services.AddSingleton<findamodel.Services.ModelPreviewService>();
builder.Services.AddSingleton<findamodel.Services.HullCalculationService>();
builder.Services.AddSingleton<findamodel.Services.MetadataConfigService>();
builder.Services.AddSingleton<findamodel.Services.ModelService>();
builder.Services.AddSingleton<findamodel.Services.ExplorerService>();
builder.Services.AddSingleton<findamodel.Services.PrintingListService>();
builder.Services.AddHostedService<findamodel.Services.ModelIndexerService>();

if (builder.Environment.IsDevelopment())
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

var app = builder.Build();

using (var scope = app.Services.CreateScope())
{
    var previewService = scope.ServiceProvider.GetRequiredService<findamodel.Services.ModelPreviewService>();
    previewService.SetCacheDirectory(cacheRendersPath);

    var dbFactory = scope.ServiceProvider.GetRequiredService<IDbContextFactory<ModelCacheContext>>();
    using var db = dbFactory.CreateDbContext();
    db.Database.EnsureCreated();
    db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;");

    var userService = scope.ServiceProvider.GetRequiredService<findamodel.Services.UserService>();
    await userService.SeedAdminUserAsync();

    var printingListService = scope.ServiceProvider.GetRequiredService<findamodel.Services.PrintingListService>();
    var adminUser = await userService.GetAdminUserAsync();
    if (adminUser != null)
        await printingListService.EnsureDefaultListAsync(adminUser.Id);
}

if (app.Environment.IsDevelopment())
{
    app.UseCors();
}

app.UseDefaultFiles();
app.UseStaticFiles();

app.UseAuthentication();
app.UseAuthorization();

app.MapControllers();

app.MapFallbackToFile("index.html");

app.Run();
