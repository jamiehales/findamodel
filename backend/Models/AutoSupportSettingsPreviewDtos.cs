using findamodel.Services;

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
    float ShrinkageEdgeBias = 0.7f,
    float ModelLiftMm = 10f,
    float OverhangSensitivity = 0.65f,
    PeelDirection PeelDirection = PeelDirection.ZPositive,
    float PeelStartMultiplier = 1.3f,
    float PeelEndMultiplier = 0.9f,
    float HeightBias = 0.3f,
    float BridgeReductionFactor = 0.3f,
    float CantileverMomentMultiplier = 0.4f,
    float CantileverReferenceLengthMm = 8f,
    float LayerBondStrengthPerMm2 = 1.2f,
    float LayerAdhesionSafetyFactor = 1.1f,
    bool SupportInteractionEnabled = true,
    float DrainageDepthForceMultiplier = 0.15f,
    bool AccessibilityEnabled = true,
    int AccessibilityScanRadiusPx = 6,
    int AccessibilityMinOpenDirections = 1,
    float SurfaceQualityWeight = 0.35f,
    int SurfaceQualitySearchRadiusPx = 6,
    bool OrientationCheckEnabled = true,
    float OrientationRiskForceMultiplierMax = 1.35f,
    float OrientationRiskThresholdRatio = 1.15f);

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
    IReadOnlyList<AutoSupportSliceLayerDto>? SliceLayers = null,
    double? GenerateMs = null,
    double? EncodeMs = null,
    double? WriteMs = null,
    double? TotalMs = null);

public sealed record AutoSupportSettingsPreviewDto(
    Guid PreviewId,
    IReadOnlyList<AutoSupportSettingsPreviewScenarioDto> Scenarios,
    double? TotalMs = null);