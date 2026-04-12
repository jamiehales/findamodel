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
    string? DirectoryFilter,
    string? RelativeModelPath,
    IndexFlags Flags,
    DateTime RequestedAt,
    string Status);  // "queued" | "running"

/// <summary>A completed indexing request retained in recent in-memory history.</summary>
public record CompletedIndexRequestDto(
    Guid Id,
    string? DirectoryFilter,
    string? RelativeModelPath,
    IndexFlags Flags,
    DateTime RequestedAt,
    DateTime StartedAt,
    DateTime CompletedAt,
    double DurationMs,
    string Outcome,
    string? Error);

/// <summary>Request body for POST /api/indexer.</summary>
public record EnqueueIndexRequest(string? DirectoryFilter, string? RelativeModelPath, IndexFlags Flags);

/// <summary>Response for GET /api/indexer.</summary>
public record IndexerStatusDto(
    bool IsRunning,
    IndexRequestDto? CurrentRequest,
    IReadOnlyList<IndexRequestDto> Queue,
    IReadOnlyList<CompletedIndexRequestDto> Recent);
