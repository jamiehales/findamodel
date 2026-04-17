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
    AutoSupportVectorDto PullForce);

public sealed record AutoSupportJobDto(
    Guid JobId,
    string Status,
    int ProgressPercent,
    int SupportCount,
    string? ErrorMessage,
    IReadOnlyList<AutoSupportPointDto>? SupportPoints = null);
