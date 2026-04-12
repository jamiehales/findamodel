using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class AppConfigService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    private const int SingletonConfigId = 1;
    public const float DatabaseDefaultRaftHeightMm = 2f;
    private static readonly HashSet<string> AllowedTagGenerationProviders = new(StringComparer.OrdinalIgnoreCase)
    {
        "internal",
        "ollama",
    };

    public async Task<float> GetDefaultRaftHeightMmAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        return config.DefaultRaftHeightMm;
    }

    public async Task<AppConfigDto> GetAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        return ToDto(config);
    }

    public async Task<AppConfigDto> UpdateAsync(UpdateAppConfigRequest request)
    {
        if (!float.IsFinite(request.DefaultRaftHeightMm) || request.DefaultRaftHeightMm < 0f)
            throw new ArgumentException("Default raft height must be a finite number greater than or equal to 0.", nameof(request.DefaultRaftHeightMm));

        var allowedThemes = new HashSet<string> { "default", "nord" };
        if (!allowedThemes.Contains(request.Theme))
            throw new ArgumentException($"Unknown theme '{request.Theme}'.", nameof(request.Theme));

        if (!AllowedTagGenerationProviders.Contains(request.TagGenerationProvider))
            throw new ArgumentException($"Unknown tag generation provider '{request.TagGenerationProvider}'.", nameof(request.TagGenerationProvider));

        if (string.IsNullOrWhiteSpace(request.TagGenerationEndpoint))
            throw new ArgumentException("Tag generation endpoint is required.", nameof(request.TagGenerationEndpoint));

        if (string.IsNullOrWhiteSpace(request.TagGenerationModel))
            throw new ArgumentException("Tag generation model is required.", nameof(request.TagGenerationModel));

        if (request.TagGenerationTimeoutMs < 1000 || request.TagGenerationTimeoutMs > 300000)
            throw new ArgumentException("Tag generation timeout must be between 1000ms and 300000ms.", nameof(request.TagGenerationTimeoutMs));

        if (request.TagGenerationMaxTags < 1 || request.TagGenerationMaxTags > TagListHelper.MaxTagCount)
            throw new ArgumentException($"Tag generation max tags must be between 1 and {TagListHelper.MaxTagCount}.", nameof(request.TagGenerationMaxTags));

        if (!float.IsFinite(request.TagGenerationMinConfidence) || request.TagGenerationMinConfidence < 0f || request.TagGenerationMinConfidence > 1f)
            throw new ArgumentException("Tag generation minimum confidence must be a finite number between 0 and 1.", nameof(request.TagGenerationMinConfidence));

        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        config.DefaultRaftHeightMm = request.DefaultRaftHeightMm;
        config.Theme = request.Theme;
        config.TagGenerationEnabled = request.TagGenerationEnabled;
        config.TagGenerationProvider = request.TagGenerationProvider.Trim().ToLowerInvariant();
        config.TagGenerationEndpoint = request.TagGenerationEndpoint.Trim();
        config.TagGenerationModel = request.TagGenerationModel.Trim();
        config.TagGenerationTimeoutMs = request.TagGenerationTimeoutMs;
        config.TagGenerationMaxTags = request.TagGenerationMaxTags;
        config.TagGenerationMinConfidence = request.TagGenerationMinConfidence;
        config.UpdatedAt = DateTime.UtcNow;
        await db.SaveChangesAsync();
        return ToDto(config);
    }

    public async Task<AppConfigDto> UpdateDefaultRaftHeightAsync(float defaultRaftHeightMm)
    {
        if (!float.IsFinite(defaultRaftHeightMm) || defaultRaftHeightMm < 0f)
            throw new ArgumentException("Default raft height must be a finite number greater than or equal to 0.", nameof(defaultRaftHeightMm));

        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        config.DefaultRaftHeightMm = defaultRaftHeightMm;
        config.UpdatedAt = DateTime.UtcNow;
        await db.SaveChangesAsync();
        return ToDto(config);
    }

    private static AppConfigDto ToDto(AppConfig config)
    {
        return new AppConfigDto(
            config.DefaultRaftHeightMm,
            config.Theme,
            config.TagGenerationEnabled,
            config.TagGenerationProvider,
            config.TagGenerationEndpoint,
            config.TagGenerationModel,
            config.TagGenerationTimeoutMs,
            config.TagGenerationMaxTags,
            config.TagGenerationMinConfidence);
    }

    private static async Task<AppConfig> EnsureConfigAsync(ModelCacheContext db)
    {
        var config = await db.AppConfigs.FirstOrDefaultAsync(c => c.Id == SingletonConfigId);
        if (config != null)
            return config;

        config = new AppConfig
        {
            Id = SingletonConfigId,
            DefaultRaftHeightMm = DatabaseDefaultRaftHeightMm,
            UpdatedAt = DateTime.UtcNow,
        };

        db.AppConfigs.Add(config);
        await db.SaveChangesAsync();
        return config;
    }
}
