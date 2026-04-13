using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class AppConfigService(IDbContextFactory<ModelCacheContext> dbFactory, IConfiguration configuration)
{
    private const int SingletonConfigId = 1;
    public const float DatabaseDefaultRaftHeightMm = 2f;
    private const string DefaultTheme = "nord";
    private const bool DefaultTagGenerationEnabled = false;
    private const bool DefaultAiDescriptionEnabled = false;
    private const string DefaultTagGenerationProvider = "internal";
    private const string DefaultTagGenerationEndpoint = "http://localhost:11434";
    private const string DefaultTagGenerationModel = "qwen2.5vl:7b";
    private const int DefaultTagGenerationTimeoutMs = 60000;
    private const int DefaultTagGenerationMaxTags = 12;
    private const float DefaultTagGenerationMinConfidence = 0.45f;
    private static readonly HashSet<string> AllowedThemes = new(StringComparer.OrdinalIgnoreCase)
    {
        "default",
        "nord",
    };
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

        if (!AllowedThemes.Contains(request.Theme))
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
        config.AiDescriptionEnabled = request.AiDescriptionEnabled;
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

    public async Task<bool> IsSetupCompletedAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await db.AppConfigs.FirstOrDefaultAsync(c => c.Id == SingletonConfigId);
        return config?.SetupCompleted ?? false;
    }

    public async Task<AppConfigDto> CompleteInitialSetupAsync(InitialSetupRequest request)
    {
        // Validate models directory path
        if (string.IsNullOrWhiteSpace(request.ModelsDirectoryPath))
            throw new ArgumentException("Models directory path is required.", nameof(request.ModelsDirectoryPath));

        var fullPath = Path.GetFullPath(request.ModelsDirectoryPath);
        if (!Directory.Exists(fullPath))
            throw new ArgumentException($"Models directory does not exist: {fullPath}", nameof(request.ModelsDirectoryPath));

        // Validate other config values
        if (!float.IsFinite(request.DefaultRaftHeightMm) || request.DefaultRaftHeightMm < 0f)
            throw new ArgumentException("Default raft height must be a finite number greater than or equal to 0.", nameof(request.DefaultRaftHeightMm));

        if (!AllowedThemes.Contains(request.Theme))
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

        config.SetupCompleted = true;
        config.ModelsDirectoryPath = fullPath;
        config.DefaultRaftHeightMm = request.DefaultRaftHeightMm;
        config.Theme = request.Theme;
        config.TagGenerationEnabled = request.TagGenerationEnabled;
        config.AiDescriptionEnabled = request.AiDescriptionEnabled;
        config.TagGenerationProvider = request.TagGenerationProvider.Trim().ToLowerInvariant();
        config.TagGenerationEndpoint = request.TagGenerationEndpoint.Trim();
        config.TagGenerationModel = request.TagGenerationModel.Trim();
        config.TagGenerationTimeoutMs = request.TagGenerationTimeoutMs;
        config.TagGenerationMaxTags = request.TagGenerationMaxTags;
        config.TagGenerationMinConfidence = request.TagGenerationMinConfidence;
        config.UpdatedAt = DateTime.UtcNow;
        configuration["Models:DirectoryPath"] = fullPath;

        await db.SaveChangesAsync();
        return ToDto(config);
    }

    public async Task<SetupStatusDto> GetSetupStatusAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        var hasConfiguredModelsPath = !string.IsNullOrWhiteSpace(config.ModelsDirectoryPath)
            || !string.IsNullOrWhiteSpace(configuration["Models:DirectoryPath"]);
        var requiresWizard = !config.SetupCompleted && !hasConfiguredModelsPath;
        return new SetupStatusDto(config.SetupCompleted, requiresWizard);
    }

    public async Task<InitialSetupDefaultsDto> GetInitialSetupDefaultsAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);

        return new InitialSetupDefaultsDto(
            ModelsDirectoryPath: !string.IsNullOrWhiteSpace(configuration["Models:DirectoryPath"]) ? configuration["Models:DirectoryPath"] : config.ModelsDirectoryPath,
            DefaultRaftHeightMm: config.DefaultRaftHeightMm,
            Theme: config.Theme,
            TagGenerationEnabled: config.TagGenerationEnabled,
            AiDescriptionEnabled: config.AiDescriptionEnabled,
            TagGenerationProvider: config.TagGenerationProvider,
            TagGenerationEndpoint: config.TagGenerationEndpoint,
            TagGenerationModel: config.TagGenerationModel,
            TagGenerationTimeoutMs: config.TagGenerationTimeoutMs,
            TagGenerationMaxTags: config.TagGenerationMaxTags,
            TagGenerationMinConfidence: config.TagGenerationMinConfidence);
    }

    private static AppConfigDto ToDto(AppConfig config)
    {
        return new AppConfigDto(
            config.DefaultRaftHeightMm,
            config.Theme,
            config.TagGenerationEnabled,
            config.AiDescriptionEnabled,
            config.TagGenerationProvider,
            config.TagGenerationEndpoint,
            config.TagGenerationModel,
            config.TagGenerationTimeoutMs,
            config.TagGenerationMaxTags,
            config.TagGenerationMinConfidence,
            config.SetupCompleted,
            config.ModelsDirectoryPath);
    }

    private async Task<AppConfig> EnsureConfigAsync(ModelCacheContext db)
    {
        var config = await db.AppConfigs.FirstOrDefaultAsync(c => c.Id == SingletonConfigId);
        if (config != null)
            return config;

        var configuredModelsPath = configuration["Models:DirectoryPath"];
        var normalizedConfiguredModelsPath = string.IsNullOrWhiteSpace(configuredModelsPath)
            ? null
            : Path.GetFullPath(configuredModelsPath);

        config = new AppConfig
        {
            Id = SingletonConfigId,
            DefaultRaftHeightMm = configuration.GetValue<float?>("AppConfig:DefaultRaftHeightMm") ?? DatabaseDefaultRaftHeightMm,
            Theme = configuration["AppConfig:Theme"] ?? DefaultTheme,
            TagGenerationEnabled = configuration.GetValue<bool?>("AppConfig:TagGenerationEnabled") ?? DefaultTagGenerationEnabled,
            AiDescriptionEnabled = configuration.GetValue<bool?>("AppConfig:AiDescriptionEnabled") ?? DefaultAiDescriptionEnabled,
            TagGenerationProvider = configuration["AppConfig:TagGenerationProvider"] ?? DefaultTagGenerationProvider,
            TagGenerationEndpoint = configuration["AppConfig:TagGenerationEndpoint"] ?? DefaultTagGenerationEndpoint,
            TagGenerationModel = configuration["AppConfig:TagGenerationModel"] ?? DefaultTagGenerationModel,
            TagGenerationTimeoutMs = configuration.GetValue<int?>("AppConfig:TagGenerationTimeoutMs") ?? DefaultTagGenerationTimeoutMs,
            TagGenerationMaxTags = configuration.GetValue<int?>("AppConfig:TagGenerationMaxTags") ?? DefaultTagGenerationMaxTags,
            TagGenerationMinConfidence = configuration.GetValue<float?>("AppConfig:TagGenerationMinConfidence") ?? DefaultTagGenerationMinConfidence,
            SetupCompleted = !string.IsNullOrWhiteSpace(normalizedConfiguredModelsPath),
            ModelsDirectoryPath = normalizedConfiguredModelsPath,
            UpdatedAt = DateTime.UtcNow,
        };

        db.AppConfigs.Add(config);
        await db.SaveChangesAsync();
        return config;
    }
}
