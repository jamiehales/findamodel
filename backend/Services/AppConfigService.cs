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
    private const string DefaultTagGenerationPromptTemplate =
        "Given the provided image and metadata context, return tags only from the allowed schema. Focus on monochrome mesh renders (no color cues). Return at most {{maxTags}} tags as JSON: {\"tags\":[...],\"confidence\":{\"tag\":0.0},\"notes\":\"optional\"}. Output the JSON object only with no leading or trailing text. Allowed tags: {{allowedTags}}.";
    private const string LegacyDescriptionGenerationPromptTemplate =
        "Write exactly two concise, searchable sentences describing this 3D model named '{{modelName}}'. Sentence 1: a general visual overview based on the image and provided metadata context. Sentence 2: key visible characteristics as a comma-separated list (for example: staff, large teeth, spikes, wings, hat, cloak, ammo, gun). Use only observable visual details and supplied metadata; do not infer gameplay role or likely use. Do not mention confidence, JSON, or model limitations. Return JSON only as {\"description\":\"...\",\"confidence\":0.0}.";
    private const string DefaultDescriptionGenerationPromptTemplate =
        "Write exactly two concise, searchable sentences describing this 3D model named '{{modelName}}', the full path is '{{fullPath}}'. Sentence 1: a general visual overview based on the image and provided metadata context. Sentence 2: key visible characteristics as a comma-separated list (for example: staff, large teeth, spikes, wings, hat, cloak, ammo, gun). Use only observable visual details and supplied metadata; do not infer gameplay role or likely use.";
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
        config.TagGenerationPromptTemplate = NormalizePromptOverride(
            request.TagGenerationPromptTemplate,
            ResolveConfiguredTagGenerationPromptTemplate());
        config.DescriptionGenerationPromptTemplate = NormalizePromptOverride(
            request.DescriptionGenerationPromptTemplate,
            ResolveConfiguredDescriptionGenerationPromptTemplate());
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

    private AppConfigDto ToDto(AppConfig config)
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
            ResolveEffectiveTagGenerationPromptTemplate(config),
            ResolveEffectiveDescriptionGenerationPromptTemplate(config),
            ResolveConfiguredTagGenerationPromptTemplate(),
            ResolveConfiguredDescriptionGenerationPromptTemplate(),
            config.TagGenerationPromptTemplate.Trim(),
            config.DescriptionGenerationPromptTemplate.Trim(),
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
            TagGenerationPromptTemplate = string.Empty,
            DescriptionGenerationPromptTemplate = string.Empty,
            SetupCompleted = !string.IsNullOrWhiteSpace(normalizedConfiguredModelsPath),
            ModelsDirectoryPath = normalizedConfiguredModelsPath,
            UpdatedAt = DateTime.UtcNow,
        };

        db.AppConfigs.Add(config);
        await db.SaveChangesAsync();
        return config;
    }

    private static string NormalizePromptOverride(string prompt, string configuredDefault)
    {
        var trimmed = prompt.Trim();
        return string.Equals(trimmed, configuredDefault, StringComparison.Ordinal)
            ? string.Empty
            : trimmed;
    }

    private string ResolveConfiguredTagGenerationPromptTemplate()
    {
        var configured = configuration["AppConfig:TagGenerationPromptTemplate"];
        return string.IsNullOrWhiteSpace(configured)
            ? DefaultTagGenerationPromptTemplate
            : configured.Trim();
    }

    private string ResolveConfiguredDescriptionGenerationPromptTemplate()
    {
        var configured = configuration["AppConfig:DescriptionGenerationPromptTemplate"];
        return string.IsNullOrWhiteSpace(configured)
            ? DefaultDescriptionGenerationPromptTemplate
            : configured.Trim();
    }

    private string ResolveEffectiveTagGenerationPromptTemplate(AppConfig config)
    {
        var configuredDefault = ResolveConfiguredTagGenerationPromptTemplate();
        var stored = config.TagGenerationPromptTemplate?.Trim();
        return !string.IsNullOrWhiteSpace(stored)
            && !string.Equals(stored, configuredDefault, StringComparison.Ordinal)
            ? stored
            : configuredDefault;
    }

    private string ResolveEffectiveDescriptionGenerationPromptTemplate(AppConfig config)
    {
        var configuredDefault = ResolveConfiguredDescriptionGenerationPromptTemplate();
        var stored = config.DescriptionGenerationPromptTemplate?.Trim();
        return !string.IsNullOrWhiteSpace(stored)
            && !string.Equals(stored, configuredDefault, StringComparison.Ordinal)
            && !string.Equals(stored, LegacyDescriptionGenerationPromptTemplate, StringComparison.Ordinal)
            && !string.Equals(stored, DefaultDescriptionGenerationPromptTemplate, StringComparison.Ordinal)
            ? stored
            : configuredDefault;
    }
}
