namespace findamodel.Services;

public enum SupportSize { Micro, Light, Medium, Heavy }

public sealed record SupportPoint(Vec3 Position, float RadiusMm, Vec3 PullForce, SupportSize Size);

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
