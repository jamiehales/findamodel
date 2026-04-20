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
    float HeavyTipRadiusMm,
    float SuctionMultiplier = 3f,
    float AreaGrowthThreshold = 0.5f,
    float AreaGrowthMultiplier = 1.5f,
    bool GravityEnabled = true,
    float ResinDensityGPerMl = 1.25f,
    float DragCoefficientMultiplier = 0.5f,
    float MinFeatureWidthMm = 1f,
    float ShrinkagePercent = 5f,
    float ShrinkageEdgeBias = 0.7f);

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
    IReadOnlyList<AutoSupportIslandDto>? Islands = null,
    double? GenerateMs = null,
    double? EncodeMs = null,
    double? WriteMs = null,
    double? TotalMs = null);

public sealed record AutoSupportSettingsPreviewDto(
    Guid PreviewId,
    IReadOnlyList<AutoSupportSettingsPreviewScenarioDto> Scenarios,
    double? TotalMs = null);