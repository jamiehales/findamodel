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
    public const string DefaultTagGenerationModel = "Qwen2.5-7B-Instruct";
    private const string DefaultTheme = "nord";
    private const bool DefaultGeneratePreviewsEnabled = true;
    private const int DefaultMinimumPreviewGenerationVersion = ModelPreviewService.CurrentPreviewGenerationVersion;
    private const bool DefaultTagGenerationEnabled = false;
    private const bool DefaultAiDescriptionEnabled = false;
    private const string DefaultTagGenerationProvider = "internal";
    private const string DefaultTagGenerationEndpoint = "http://localhost:11434";
    private const int DefaultTagGenerationTimeoutMs = 60000;
    private const int DefaultTagGenerationMaxTags = 12;
    private const float DefaultTagGenerationMinConfidence = 0.45f;
    public const float DefaultAutoSupportBedMarginMm = 2f;
    public const float DefaultAutoSupportMinVoxelSizeMm = 0.8f;
    public const float DefaultAutoSupportMaxVoxelSizeMm = 2f;
    public const float DefaultAutoSupportMinLayerHeightMm = 0.75f;
    public const float DefaultAutoSupportMaxLayerHeightMm = 1.5f;
    public const float DefaultAutoSupportMergeDistanceMm = 2.5f;
    public const float DefaultAutoSupportPullForceThreshold = 3f;
    public const float DefaultAutoSupportSphereRadiusMm = 1.2f;
    public const int DefaultAutoSupportMaxSupportsPerIsland = 6;
    public const string DefaultTagGenerationPromptTemplate =
        "Given the provided image and metadata context, return tags only from the allowed schema. Focus on monochrome mesh renders (no color cues). Return at most {{maxTags}} tags as JSON: {\"tags\":[...],\"confidence\":{\"tag\":0.0},\"notes\":\"optional\"}. Output the JSON object only with no leading or trailing text. Allowed tags: {{allowedTags}}.";
    public const string DefaultDescriptionGenerationPromptTemplate =
        "You are an image analyzer. Your role is to look at the image provided, and generate a list of keywords based on it. Do not refer to color or brightness in any way. Following are examples of keywords that could be used. Type of object: animal, object, person, humanoid. Characteristics: cute, ugly, round, straight. Name: lamp, dog, man, woman, monster, cat. Actions: shooting, ducking, jumping, lying, dancing. An example for a humanoid lizard that's a guard, looking out for intruders, with a parrot on his shoulder would be: lizard guarding anxious crossbow arrows crouching parrot bird animal humanoid guard fat armor smooth boots hat feather. Do not limit yourself to these specific keywords. If something looks like two different things, use both. Respond with the top 25 keywords, be verbose. Use spaces to separate the keywords, and only use lowercase. Respond with ONLY human readable text, ZERO formatting, markup or punctuation.";
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

        ValidateMinimumPreviewGenerationVersion(request.MinimumPreviewGenerationVersion, nameof(request.MinimumPreviewGenerationVersion));

        if (!AllowedTagGenerationProviders.Contains(request.TagGenerationProvider))
            throw new ArgumentException($"Unknown tag generation provider '{request.TagGenerationProvider}'.", nameof(request.TagGenerationProvider));

        if (string.IsNullOrWhiteSpace(request.TagGenerationEndpoint))
            throw new ArgumentException("Tag generation endpoint is required.", nameof(request.TagGenerationEndpoint));

        if (request.TagGenerationTimeoutMs < 1000 || request.TagGenerationTimeoutMs > 300000)
            throw new ArgumentException("Tag generation timeout must be between 1000ms and 300000ms.", nameof(request.TagGenerationTimeoutMs));

        if (request.TagGenerationMaxTags < 1 || request.TagGenerationMaxTags > TagListHelper.MaxTagCount)
            throw new ArgumentException($"Tag generation max tags must be between 1 and {TagListHelper.MaxTagCount}.", nameof(request.TagGenerationMaxTags));

        if (!float.IsFinite(request.TagGenerationMinConfidence) || request.TagGenerationMinConfidence < 0f || request.TagGenerationMinConfidence > 1f)
            throw new ArgumentException("Tag generation minimum confidence must be a finite number between 0 and 1.", nameof(request.TagGenerationMinConfidence));

        ValidateAutoSupportSettings(request);

        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        config.DefaultRaftHeightMm = request.DefaultRaftHeightMm;
        config.Theme = request.Theme;
        config.GeneratePreviewsEnabled = request.GeneratePreviewsEnabled;
        config.MinimumPreviewGenerationVersion = request.MinimumPreviewGenerationVersion;
        config.TagGenerationEnabled = request.TagGenerationEnabled;
        config.AiDescriptionEnabled = request.AiDescriptionEnabled;
        config.TagGenerationProvider = request.TagGenerationProvider.Trim().ToLowerInvariant();
        config.TagGenerationEndpoint = request.TagGenerationEndpoint.Trim();
        config.TagGenerationModel = NormalizeModelOverride(
            request.TagGenerationModel,
            ResolveConfiguredTagGenerationModel(request.TagGenerationProvider));
        config.TagGenerationTimeoutMs = request.TagGenerationTimeoutMs;
        config.TagGenerationMaxTags = request.TagGenerationMaxTags;
        config.TagGenerationMinConfidence = request.TagGenerationMinConfidence;
        config.TagGenerationPromptTemplate = NormalizePromptOverride(
            request.TagGenerationPromptTemplate,
            ResolveConfiguredTagGenerationPromptTemplate());
        config.DescriptionGenerationPromptTemplate = NormalizePromptOverride(
            request.DescriptionGenerationPromptTemplate,
            ResolveConfiguredDescriptionGenerationPromptTemplate());
        config.AutoSupportBedMarginMm = request.AutoSupportBedMarginMm;
        config.AutoSupportMinVoxelSizeMm = request.AutoSupportMinVoxelSizeMm;
        config.AutoSupportMaxVoxelSizeMm = request.AutoSupportMaxVoxelSizeMm;
        config.AutoSupportMinLayerHeightMm = request.AutoSupportMinLayerHeightMm;
        config.AutoSupportMaxLayerHeightMm = request.AutoSupportMaxLayerHeightMm;
        config.AutoSupportMergeDistanceMm = request.AutoSupportMergeDistanceMm;
        config.AutoSupportPullForceThreshold = request.AutoSupportPullForceThreshold;
        config.AutoSupportSphereRadiusMm = request.AutoSupportSphereRadiusMm;
        config.AutoSupportMaxSupportsPerIsland = request.AutoSupportMaxSupportsPerIsland;
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
        config.GeneratePreviewsEnabled = request.GeneratePreviewsEnabled;
        config.TagGenerationEnabled = request.TagGenerationEnabled;
        config.AiDescriptionEnabled = request.AiDescriptionEnabled;
        config.TagGenerationProvider = request.TagGenerationProvider.Trim().ToLowerInvariant();
        config.TagGenerationEndpoint = request.TagGenerationEndpoint.Trim();
        config.TagGenerationModel = NormalizeModelOverride(
            request.TagGenerationModel,
            ResolveConfiguredTagGenerationModel(request.TagGenerationProvider));
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
            GeneratePreviewsEnabled: config.GeneratePreviewsEnabled,
            TagGenerationEnabled: config.TagGenerationEnabled,
            AiDescriptionEnabled: config.AiDescriptionEnabled,
            TagGenerationProvider: config.TagGenerationProvider,
            TagGenerationEndpoint: config.TagGenerationEndpoint,
            TagGenerationModel: ResolveEffectiveTagGenerationModel(config),
            TagGenerationTimeoutMs: config.TagGenerationTimeoutMs,
            TagGenerationMaxTags: config.TagGenerationMaxTags,
            TagGenerationMinConfidence: config.TagGenerationMinConfidence);
    }

    private AppConfigDto ToDto(AppConfig config)
    {
        return new AppConfigDto(
            config.DefaultRaftHeightMm,
            config.Theme,
            config.GeneratePreviewsEnabled,
            NormalizeMinimumPreviewGenerationVersion(config.MinimumPreviewGenerationVersion),
            config.TagGenerationEnabled,
            config.AiDescriptionEnabled,
            config.TagGenerationProvider,
            config.TagGenerationEndpoint,
            ResolveEffectiveTagGenerationModel(config),
            ResolveConfiguredTagGenerationModel(config.TagGenerationProvider),
            config.TagGenerationModel?.Trim() ?? string.Empty,
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
            config.ModelsDirectoryPath,
            config.AutoSupportBedMarginMm,
            config.AutoSupportMinVoxelSizeMm,
            config.AutoSupportMaxVoxelSizeMm,
            config.AutoSupportMinLayerHeightMm,
            config.AutoSupportMaxLayerHeightMm,
            config.AutoSupportMergeDistanceMm,
            config.AutoSupportPullForceThreshold,
            config.AutoSupportSphereRadiusMm,
            config.AutoSupportMaxSupportsPerIsland);
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
            GeneratePreviewsEnabled = configuration.GetValue<bool?>("AppConfig:GeneratePreviewsEnabled") ?? DefaultGeneratePreviewsEnabled,
            MinimumPreviewGenerationVersion = ResolveConfiguredMinimumPreviewGenerationVersion(),
            TagGenerationEnabled = configuration.GetValue<bool?>("AppConfig:TagGenerationEnabled") ?? DefaultTagGenerationEnabled,
            AiDescriptionEnabled = configuration.GetValue<bool?>("AppConfig:AiDescriptionEnabled") ?? DefaultAiDescriptionEnabled,
            TagGenerationProvider = configuration["AppConfig:TagGenerationProvider"] ?? DefaultTagGenerationProvider,
            TagGenerationEndpoint = configuration["AppConfig:TagGenerationEndpoint"] ?? DefaultTagGenerationEndpoint,
            TagGenerationModel = null,
            TagGenerationTimeoutMs = configuration.GetValue<int?>("AppConfig:TagGenerationTimeoutMs") ?? DefaultTagGenerationTimeoutMs,
            TagGenerationMaxTags = configuration.GetValue<int?>("AppConfig:TagGenerationMaxTags") ?? DefaultTagGenerationMaxTags,
            TagGenerationMinConfidence = configuration.GetValue<float?>("AppConfig:TagGenerationMinConfidence") ?? DefaultTagGenerationMinConfidence,
            TagGenerationPromptTemplate = string.Empty,
            DescriptionGenerationPromptTemplate = string.Empty,
            AutoSupportBedMarginMm = configuration.GetValue<float?>("AppConfig:AutoSupportBedMarginMm") ?? DefaultAutoSupportBedMarginMm,
            AutoSupportMinVoxelSizeMm = configuration.GetValue<float?>("AppConfig:AutoSupportMinVoxelSizeMm") ?? DefaultAutoSupportMinVoxelSizeMm,
            AutoSupportMaxVoxelSizeMm = configuration.GetValue<float?>("AppConfig:AutoSupportMaxVoxelSizeMm") ?? DefaultAutoSupportMaxVoxelSizeMm,
            AutoSupportMinLayerHeightMm = configuration.GetValue<float?>("AppConfig:AutoSupportMinLayerHeightMm") ?? DefaultAutoSupportMinLayerHeightMm,
            AutoSupportMaxLayerHeightMm = configuration.GetValue<float?>("AppConfig:AutoSupportMaxLayerHeightMm") ?? DefaultAutoSupportMaxLayerHeightMm,
            AutoSupportMergeDistanceMm = configuration.GetValue<float?>("AppConfig:AutoSupportMergeDistanceMm") ?? DefaultAutoSupportMergeDistanceMm,
            AutoSupportPullForceThreshold = configuration.GetValue<float?>("AppConfig:AutoSupportPullForceThreshold") ?? DefaultAutoSupportPullForceThreshold,
            AutoSupportSphereRadiusMm = configuration.GetValue<float?>("AppConfig:AutoSupportSphereRadiusMm") ?? DefaultAutoSupportSphereRadiusMm,
            AutoSupportMaxSupportsPerIsland = configuration.GetValue<int?>("AppConfig:AutoSupportMaxSupportsPerIsland") ?? DefaultAutoSupportMaxSupportsPerIsland,
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

    private static void ValidateMinimumPreviewGenerationVersion(int minimumPreviewGenerationVersion, string paramName)
    {
        if (minimumPreviewGenerationVersion < 0 || minimumPreviewGenerationVersion > ModelPreviewService.CurrentPreviewGenerationVersion)
            throw new ArgumentException(
                $"Minimum preview version must be between 0 and {ModelPreviewService.CurrentPreviewGenerationVersion}.",
                paramName);
    }

    private static void ValidateAutoSupportSettings(UpdateAppConfigRequest request)
    {
        ValidateFiniteRange(request.AutoSupportBedMarginMm, 0f, 20f, nameof(request.AutoSupportBedMarginMm));
        ValidateFiniteRange(request.AutoSupportMinVoxelSizeMm, 0.1f, 10f, nameof(request.AutoSupportMinVoxelSizeMm));
        ValidateFiniteRange(request.AutoSupportMaxVoxelSizeMm, request.AutoSupportMinVoxelSizeMm, 10f, nameof(request.AutoSupportMaxVoxelSizeMm));
        ValidateFiniteRange(request.AutoSupportMinLayerHeightMm, 0.05f, 10f, nameof(request.AutoSupportMinLayerHeightMm));
        ValidateFiniteRange(request.AutoSupportMaxLayerHeightMm, request.AutoSupportMinLayerHeightMm, 10f, nameof(request.AutoSupportMaxLayerHeightMm));
        ValidateFiniteRange(request.AutoSupportMergeDistanceMm, 0.1f, 25f, nameof(request.AutoSupportMergeDistanceMm));
        ValidateFiniteRange(request.AutoSupportPullForceThreshold, 0.1f, 100f, nameof(request.AutoSupportPullForceThreshold));
        ValidateFiniteRange(request.AutoSupportSphereRadiusMm, 0.1f, 10f, nameof(request.AutoSupportSphereRadiusMm));

        if (request.AutoSupportMaxSupportsPerIsland < 1 || request.AutoSupportMaxSupportsPerIsland > 64)
            throw new ArgumentException("Auto support max supports per island must be between 1 and 64.", nameof(request.AutoSupportMaxSupportsPerIsland));
    }

    private static void ValidateFiniteRange(float value, float min, float max, string paramName)
    {
        if (!float.IsFinite(value) || value < min || value > max)
            throw new ArgumentException($"{paramName} must be a finite number between {min} and {max}.", paramName);
    }

    private static int NormalizeMinimumPreviewGenerationVersion(int minimumPreviewGenerationVersion) =>
        Math.Clamp(minimumPreviewGenerationVersion, 0, ModelPreviewService.CurrentPreviewGenerationVersion);

    private int ResolveConfiguredMinimumPreviewGenerationVersion() =>
        NormalizeMinimumPreviewGenerationVersion(
            configuration.GetValue<int?>("AppConfig:MinimumPreviewGenerationVersion") ?? DefaultMinimumPreviewGenerationVersion);

    private static string? NormalizeModelOverride(string model, string configuredDefault)
    {
        if (string.IsNullOrWhiteSpace(model))
            return null;

        var trimmed = model.Trim();
        return string.Equals(trimmed, configuredDefault, StringComparison.Ordinal)
            ? null
            : trimmed;
    }

    public static string GetDefaultTagGenerationModel() => DefaultTagGenerationModel;

    public static bool IsAnyAiGenerationEnabled(AppConfigDto? config) =>
        config is not null && (config.TagGenerationEnabled || config.AiDescriptionEnabled);

    private string ResolveConfiguredTagGenerationModel(string? provider = null)
    {
        var configured = configuration["AppConfig:TagGenerationModel"];
        return string.IsNullOrWhiteSpace(configured)
            ? GetDefaultTagGenerationModel()
            : configured.Trim();
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

    private string ResolveEffectiveTagGenerationModel(AppConfig config)
    {
        var configuredDefault = ResolveConfiguredTagGenerationModel(config.TagGenerationProvider);
        var stored = config.TagGenerationModel?.Trim();
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
            && !string.Equals(stored, DefaultDescriptionGenerationPromptTemplate, StringComparison.Ordinal)
            ? stored
            : configuredDefault;
    }
}
