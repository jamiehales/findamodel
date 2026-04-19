namespace findamodel.Models;

public sealed record AutoSupportSettingsPreviewTuningRequest(
    float BedMarginMm,
    float MinVoxelSizeMm,
    float MaxVoxelSizeMm,
    float MinLayerHeightMm,
    float MaxLayerHeightMm,
    float MergeDistanceMm,
    float MinIslandAreaMm2,
    float ResinStrength,
    float CrushForceThreshold,
    float MaxAngularForce,
    float PeelForceMultiplier,
    float LightTipRadiusMm,
    float MediumTipRadiusMm,
    float HeavyTipRadiusMm);

public sealed record AutoSupportSettingsPreviewRequest(
    AutoSupportSettingsPreviewTuningRequest Tuning,
    string? ScenarioId = null);

public sealed record AutoSupportSettingsPreviewScenarioDto(
    string ScenarioId,
    string Name,
    string Source,
    string Status,
    int SupportCount,
    string? ErrorMessage,
    IReadOnlyList<AutoSupportPointDto>? SupportPoints = null,
    IReadOnlyList<AutoSupportIslandDto>? Islands = null);

public sealed record AutoSupportSettingsPreviewDto(
    Guid PreviewId,
    IReadOnlyList<AutoSupportSettingsPreviewScenarioDto> Scenarios);