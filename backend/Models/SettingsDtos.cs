namespace findamodel.Models;

public record AppConfigDto(
    float DefaultRaftHeightMm,
    string Theme,
    bool TagGenerationEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    bool TagGenerationAutoApply,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence);

public record UpdateAppConfigRequest(
    float DefaultRaftHeightMm,
    string Theme,
    bool TagGenerationEnabled,
    string TagGenerationProvider,
    string TagGenerationEndpoint,
    string TagGenerationModel,
    int TagGenerationTimeoutMs,
    bool TagGenerationAutoApply,
    int TagGenerationMaxTags,
    float TagGenerationMinConfidence);
