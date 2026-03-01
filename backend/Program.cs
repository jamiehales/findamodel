using findamodel.Data;
using Microsoft.EntityFrameworkCore;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddControllers();

builder.Services.AddDbContextFactory<ModelCacheContext>(options =>
    options.UseSqlite(builder.Configuration.GetConnectionString("ModelCache") ?? "Data Source=models_cache.db"));

builder.Services.AddSingleton<findamodel.Services.ModelLoaderService>();
builder.Services.AddSingleton<findamodel.Services.ModelSaverService>();
builder.Services.AddSingleton<findamodel.Services.ModelPreviewService>();
builder.Services.AddSingleton<findamodel.Services.HullCalculationService>();
builder.Services.AddSingleton<findamodel.Services.MetadataConfigService>();
builder.Services.AddSingleton<findamodel.Services.ModelService>();
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
    var dbFactory = scope.ServiceProvider.GetRequiredService<IDbContextFactory<ModelCacheContext>>();
    using var db = dbFactory.CreateDbContext();
    db.Database.EnsureCreated();
    db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;");
}

if (app.Environment.IsDevelopment())
{
    app.UseCors();
}

app.UseDefaultFiles();
app.UseStaticFiles();

app.MapControllers();

app.MapFallbackToFile("index.html");

app.Run();
