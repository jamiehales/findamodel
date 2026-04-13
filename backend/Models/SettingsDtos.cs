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
    float TagGenerationMinConfidence);

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
