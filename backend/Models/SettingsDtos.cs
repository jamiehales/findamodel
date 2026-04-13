namespace findamodel.Models;

public record AppConfigDto(
    float DefaultRaftHeightMm,
    string Theme,
    bool TagGenerationEnabled,
    bool AiDescriptionEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence,
    bool SetupCompleted,
    string? ModelsDirectoryPath);

public record UpdateAppConfigRequest(
    float DefaultRaftHeightMm,
    string Theme,
    bool TagGenerationEnabled,
    bool AiDescriptionEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence);

public record InitialSetupRequest(
    string ModelsDirectoryPath,
    float DefaultRaftHeightMm,
    string Theme,
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
    bool InternalLlmGpuEnabled,
    int InternalLlmGpuLayerCount,
    int ModelCount,
    int ModelsWithPreviews,
    int ModelsWithGeneratedTags,
    int ModelsWithGeneratedDescriptions,
    int DirectoryConfigCount,
    int PrintingListCount,
    int MetadataDictionaryValueCount);
