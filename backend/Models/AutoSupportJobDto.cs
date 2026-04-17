namespace findamodel.Models;

public sealed record AutoSupportJobDto(
    Guid JobId,
    string Status,
    int ProgressPercent,
    int SupportCount,
    string? ErrorMessage);
