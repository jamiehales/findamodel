namespace findamodel.Models;

public sealed record AutoSupportVectorDto(
    float X,
    float Y,
    float Z);

public sealed record AutoSupportLayerForceDto(
    int LayerIndex,
    float SliceHeightMm,
    AutoSupportVectorDto Gravity,
    AutoSupportVectorDto Peel,
    AutoSupportVectorDto Rotation,
    AutoSupportVectorDto Total);

public sealed record AutoSupportPointDto(
    float X,
    float Y,
    float Z,
    float RadiusMm,
    AutoSupportVectorDto PullForce,
    string Size,
    IReadOnlyList<AutoSupportLayerForceDto>? LayerForces = null);

public sealed record AutoSupportIslandDto(
    float CentroidX,
    float CentroidZ,
    float SliceHeightMm,
    float AreaMm2,
    float RadiusMm,
    IReadOnlyList<AutoSupportVectorDto>? Boundary = null);

public sealed record AutoSupportSliceLayerDto(
    int LayerIndex,
    float SliceHeightMm,
    IReadOnlyList<AutoSupportIslandDto> Islands,
    float BedWidthMm,
    float BedDepthMm,
    string? SliceMaskPngBase64 = null);

public sealed record AutoSupportJobDto(
    Guid JobId,
    string Status,
    int ProgressPercent,
    int SupportCount,
    string? ErrorMessage,
    IReadOnlyList<AutoSupportPointDto>? SupportPoints = null,
    IReadOnlyList<AutoSupportIslandDto>? Islands = null,
    IReadOnlyList<AutoSupportSliceLayerDto>? SliceLayers = null);

