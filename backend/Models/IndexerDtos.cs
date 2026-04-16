namespace findamodel.Models;

[Flags]
public enum IndexFlags
{
    None = 0,
    Directories = 1,
    Models = 2,
}

/// <summary>A snapshot of a single indexing request for API responses.</summary>
public record IndexRequestDto(
    Guid Id,
    Guid? RunId,
    string? DirectoryFilter,
    string? RelativeModelPath,
    IndexFlags Flags,
    DateTime RequestedAt,
    int? TotalFiles,
    int ProcessedFiles,
    string Status,
    bool IsCancellationRequested);  // "queued" | "running"

/// <summary>A completed indexing request retained in recent in-memory history.</summary>
public record CompletedIndexRequestDto(
    Guid Id,
    Guid? RunId,
    string? DirectoryFilter,
    string? RelativeModelPath,
    IndexFlags Flags,
    DateTime RequestedAt,
    DateTime StartedAt,
    DateTime CompletedAt,
    double DurationMs,
    string Outcome,
    string? Error);

public record IndexRunSummaryDto(
    Guid Id,
    string? DirectoryFilter,
    string? RelativeModelPath,
    IndexFlags Flags,
    DateTime RequestedAt,
    DateTime? StartedAt,
    DateTime? CompletedAt,
    int? TotalFiles,
    int ProcessedFiles,
    string Status,
    string? Outcome,
    string? Error,
    bool IsCancellationRequested);

public record IndexRunFileDto(
    string RelativePath,
    string FileType,
    string Status,
    bool IsNew,
    bool WasUpdated,
    bool GeneratedPreview,
    bool GeneratedHull,
    bool GeneratedAiTags,
    bool GeneratedAiDescription,
    string? AiGenerationReason,
    string? Message,
    double? DurationMs,
    DateTime? ProcessedAt);

public record PagedResultDto<T>(
    IReadOnlyList<T> Items,
    int Page,
    int PageSize,
    int TotalCount);

public record IndexRunEventDto(DateTime CreatedAt, string Level, string Message, string? RelativePath);

public record IndexRunDetailDto(
    IndexRunSummaryDto Run,
    PagedResultDto<IndexRunFileDto> Files,
    PagedResultDto<IndexRunEventDto> Events);

/// <summary>Request body for POST /api/indexer.</summary>
public record EnqueueIndexRequest(string? DirectoryFilter, string? RelativeModelPath, IndexFlags Flags);

/// <summary>Response for GET /api/indexer.</summary>
public record IndexerStatusDto(
    bool IsRunning,
    IndexRequestDto? CurrentRequest,
    IReadOnlyList<IndexRequestDto> Queue,
    IReadOnlyList<CompletedIndexRequestDto> Recent);
