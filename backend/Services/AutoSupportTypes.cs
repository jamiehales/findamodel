namespace findamodel.Services;

public enum SupportSize { Micro, Light, Medium, Heavy }

public sealed record SupportPoint(Vec3 Position, float RadiusMm, Vec3 PullForce, SupportSize Size);

public sealed record AutoSupportV3TuningOverrides(
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
    float HeavyTipRadiusMm);

public sealed record SupportPreviewResult(
    IReadOnlyList<SupportPoint> SupportPoints,
    LoadedGeometry SupportGeometry,
    IReadOnlyList<IslandPreview> Islands,
    LoadedGeometry? BodyGeometry = null);

public sealed record IslandPreview(
    float CentroidX,
    float CentroidZ,
    float SliceHeightMm,
    float AreaMm2,
    float RadiusMm,
    IReadOnlyList<(float X, float Z)>? Boundary = null);
