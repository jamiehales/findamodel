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
    IndexFlags Flags,
    DateTime RequestedAt,
    string Status);  // "queued" | "running"

/// <summary>Request body for POST /api/indexer.</summary>
public record EnqueueIndexRequest(string? DirectoryFilter, IndexFlags Flags);

/// <summary>Response for GET /api/indexer.</summary>
public record IndexerStatusDto(
    bool IsRunning,
    IndexRequestDto? CurrentRequest,
    IReadOnlyList<IndexRequestDto> Queue);
