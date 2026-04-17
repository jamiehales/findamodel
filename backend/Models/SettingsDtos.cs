namespace findamodel.Models;

public record AppConfigDto(
    float DefaultRaftHeightMm,
    string Theme,
    bool GeneratePreviewsEnabled,
    int MinimumPreviewGenerationVersion,
    bool TagGenerationEnabled,
    bool AiDescriptionEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    string TagGenerationModelDefault,
    string TagGenerationModelOverride,
    int TagGenerationTimeoutMs,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence,
    string TagGenerationPromptTemplate,
    string DescriptionGenerationPromptTemplate,
    string TagGenerationPromptTemplateDefault,
    string DescriptionGenerationPromptTemplateDefault,
    string TagGenerationPromptTemplateOverride,
    string DescriptionGenerationPromptTemplateOverride,
    bool SetupCompleted,
    string? ModelsDirectoryPath,
    float AutoSupportBedMarginMm = 2f,
    float AutoSupportMinVoxelSizeMm = 0.8f,
    float AutoSupportMaxVoxelSizeMm = 2f,
    float AutoSupportMinLayerHeightMm = 0.75f,
    float AutoSupportMaxLayerHeightMm = 1.5f,
    float AutoSupportMergeDistanceMm = 2.5f,
    float AutoSupportPullForceThreshold = 3f,
    float AutoSupportSphereRadiusMm = 1.2f,
    int AutoSupportMaxSupportsPerIsland = 6,
    float AutoSupportMinIslandAreaMm2 = 4f,
    float AutoSupportMaxSupportDistanceMm = 10f);

public record UpdateAppConfigRequest(
    float DefaultRaftHeightMm,
    string Theme,
    bool GeneratePreviewsEnabled,
    int MinimumPreviewGenerationVersion,
    bool TagGenerationEnabled,
    bool AiDescriptionEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence,
    string TagGenerationPromptTemplate,
    string DescriptionGenerationPromptTemplate,
    float AutoSupportBedMarginMm = 2f,
    float AutoSupportMinVoxelSizeMm = 0.8f,
    float AutoSupportMaxVoxelSizeMm = 2f,
    float AutoSupportMinLayerHeightMm = 0.75f,
    float AutoSupportMaxLayerHeightMm = 1.5f,
    float AutoSupportMergeDistanceMm = 2.5f,
    float AutoSupportPullForceThreshold = 3f,
    float AutoSupportSphereRadiusMm = 1.2f,
    int AutoSupportMaxSupportsPerIsland = 6,
    float AutoSupportMinIslandAreaMm2 = 4f,
    float AutoSupportMaxSupportDistanceMm = 10f);

public record InitialSetupRequest(
    string ModelsDirectoryPath,
    float DefaultRaftHeightMm,
    string Theme,
    bool GeneratePreviewsEnabled,
    bool TagGenerationEnabled,
    bool AiDescriptionEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence);

public record SetupStatusDto(
    bool SetupCompleted,
    bool RequiresWizard);

public record InitialSetupDefaultsDto(
    string? ModelsDirectoryPath,
    float DefaultRaftHeightMm,
    string Theme,
    bool GeneratePreviewsEnabled,
    bool TagGenerationEnabled,
    bool AiDescriptionEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence);

public record ApplicationLogEntryDto(
    DateTimeOffset Timestamp,
    string Severity,
    string Channel,
    string Message,
    string? Exception);

public record ApplicationLogsResponseDto(
    IReadOnlyList<ApplicationLogEntryDto> Entries,
    IReadOnlyList<string> AvailableChannels,
    IReadOnlyList<string> AvailableSeverities);

public record InstanceStatsDto(
    string ApplicationVersion,
    string Environment,
    string FrameworkVersion,
    string OperatingSystem,
    int PreviewGenerationVersion,
    int HullGenerationVersion,
    bool PreviewGpuEnabled,
    bool PreviewGpuAvailable,
    string PreviewRenderer,
    bool InternalLlmGpuEnabled,
    int InternalLlmGpuLayerCount,
    int ModelCount,
    int ModelsWithPreviews,
    int ModelsWithGeneratedTags,
    int ModelsWithGeneratedDescriptions,
    int DirectoryConfigCount,
    int PrintingListCount,
    int MetadataDictionaryValueCount);
