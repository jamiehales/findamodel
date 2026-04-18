namespace findamodel.Models;

public sealed record AutoSupportVectorDto(
    float X,
    float Y,
    float Z);

public sealed record AutoSupportPointDto(
    float X,
    float Y,
    float Z,
    float RadiusMm,
    AutoSupportVectorDto PullForce,
    string Size);

public sealed record AutoSupportIslandDto(
    float CentroidX,
    float CentroidZ,
    float SliceHeightMm,
    float AreaMm2,
    float RadiusMm);

public sealed record AutoSupportJobDto(
    Guid JobId,
    string Status,
    int ProgressPercent,
    int SupportCount,
    string? ErrorMessage,
    IReadOnlyList<AutoSupportPointDto>? SupportPoints = null,
    IReadOnlyList<AutoSupportIslandDto>? Islands = null);

public sealed record CreateAutoSupportJobRequest(int Method = 1);
