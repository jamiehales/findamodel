namespace findamodel.Services;

public enum SupportSize { Micro, Light, Medium, Heavy }

public enum PeelDirection
{
    XPositive,
    XNegative,
    ZPositive,
    ZNegative,
}

public sealed record SupportLayerForce(
    int LayerIndex,
    float SliceHeightMm,
    Vec3 Gravity,
    Vec3 Peel,
    Vec3 Rotation,
    Vec3 Total);

public sealed record SupportPoint(
    Vec3 Position,
    float RadiusMm,
    Vec3 PullForce,
    SupportSize Size,
    IReadOnlyList<SupportLayerForce>? LayerForces = null);

public sealed record AutoSupportTuningOverrides(
    float BedMarginMm,
    float MinVoxelSizeMm,
    float MaxVoxelSizeMm,
    float MinLayerHeightMm,
    float MaxLayerHeightMm,
    float MinIslandAreaMm2,
    float SupportSpacingThresholdMm,
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

public sealed record SupportPreviewResult(
    IReadOnlyList<SupportPoint> SupportPoints,
    LoadedGeometry SupportGeometry,
    IReadOnlyList<IslandPreview> Islands,
    IReadOnlyList<SliceLayerPreview>? SliceLayers = null,
    LoadedGeometry? BodyGeometry = null);

public sealed record IslandPreview(
    float CentroidX,
    float CentroidZ,
    float SliceHeightMm,
    float AreaMm2,
    float RadiusMm,
    IReadOnlyList<(float X, float Z)>? Boundary = null);

public sealed record SliceLayerPreview(
    int LayerIndex,
    float SliceHeightMm,
    IReadOnlyList<IslandPreview> Islands,
    float BedWidthMm,
    float BedDepthMm,
    string? SliceMaskPngBase64 = null);
