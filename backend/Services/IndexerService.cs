using findamodel.Models;

namespace findamodel.Services;

/// <summary>
/// Manages a prioritised queue of indexing requests and processes them one at a time
/// on a background task. Keeps all scanning logic inside ModelService and
/// MetadataConfigService; only orchestrates dispatch and sequencing.
/// </summary>
public class IndexerService(
    MetadataConfigService metadataConfigService,
    ModelService modelService,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Indexing);
    private readonly object _lock = new();
    private readonly LinkedList<IndexEntry> _queue = new();
    private readonly LinkedList<CompletedIndexEntry> _recent = new();
    private const int RecentLimit = 15;
    private IndexEntry? _current;
    private Task? _processingTask;

    // -------------------------------------------------------------------------
    // Public API
    // -------------------------------------------------------------------------

    /// <summary>
    /// Adds an indexing request to the queue.
    /// If a request for the same target is already queued,
    /// its flags are merged and it is moved to the front of the queue.
    /// </summary>
    public IndexRequestDto Enqueue(string? directoryFilter, string? relativeModelPath, IndexFlags flags)
    {
        lock (_lock)
        {
            // Normalise empty string → null so root-level requests deduplicate correctly.
            var filter = string.IsNullOrEmpty(directoryFilter) ? null : directoryFilter;
            var modelPath = string.IsNullOrEmpty(relativeModelPath) ? null : relativeModelPath;

            var existing = FindQueued(filter, modelPath);
            if (existing != null)
            {
                existing.Flags |= flags;
                _queue.Remove(existing.Node!);
                _queue.AddFirst(existing.Node!);
                return existing.ToDto("queued");
            }

            var entry = new IndexEntry(filter, modelPath, flags);
            entry.Node = _queue.AddLast(entry);

            if (_processingTask == null || _processingTask.IsCompleted)
                _processingTask = Task.Run(ProcessQueueAsync);

            return entry.ToDto("queued");
        }
    }

    /// <summary>Returns a point-in-time snapshot of the current queue state.</summary>
    public IndexerStatusDto GetStatus()
    {
        lock (_lock)
        {
            var currentDto = _current?.ToDto("running");
            var queueDtos = _queue.Select(e => e.ToDto("queued")).ToList();
            var recentDtos = _recent.Select(e => e.ToDto()).ToList();
            return new IndexerStatusDto(_current != null, currentDto, queueDtos, recentDtos);
        }
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    private IndexEntry? FindQueued(string? filter, string? modelPath)
    {
        foreach (var entry in _queue)
            if (entry.DirectoryFilter == filter && entry.RelativeModelPath == modelPath)
                return entry;
        return null;
    }

    private async Task ProcessQueueAsync()
    {
        while (true)
        {
            IndexEntry entry;
            lock (_lock)
            {
                if (_queue.Count == 0)
                {
                    _current = null;
                    _processingTask = null;
                    return;
                }
                entry = _queue.First!.Value;
                _queue.RemoveFirst();
                _current = entry;
            }

            var completed = await ProcessRequestAsync(entry);

            lock (_lock)
            {
                _current = null;
                _recent.AddFirst(completed);
                while (_recent.Count > RecentLimit)
                    _recent.RemoveLast();
            }
        }
    }

    private async Task<CompletedIndexEntry> ProcessRequestAsync(IndexEntry entry)
    {
        var label = entry.RelativeModelPath is not null
            ? $"model:{entry.RelativeModelPath}"
            : entry.DirectoryFilter ?? "(all)";
        var startedAt = DateTime.UtcNow;
        var completedAt = startedAt;
        var outcome = "success";
        string? error = null;

        logger.LogInformation("IndexerService: starting {Flags} for {Target}", entry.Flags, label);

        try
        {
            if (entry.Flags.HasFlag(IndexFlags.Directories))
                await metadataConfigService.SyncDirectoryConfigsAsync();

            if (entry.Flags.HasFlag(IndexFlags.Models))
            {
                if (entry.RelativeModelPath is not null)
                    await modelService.ScanAndCacheSingleAsync(entry.RelativeModelPath);
                else
                    await modelService.ScanAndCacheAsync(directoryFilter: entry.DirectoryFilter);
            }

            logger.LogInformation("IndexerService: completed {Flags} for {Target}", entry.Flags, label);
        }
        catch (Exception ex)
        {
            outcome = "failed";
            error = SummarizeError(ex);
            logger.LogError(ex, "IndexerService: failed processing {Flags} for {Target}", entry.Flags, label);
        }

        completedAt = DateTime.UtcNow;

        var durationMs = Math.Max(0, (completedAt - startedAt).TotalMilliseconds);
        return new CompletedIndexEntry(
            entry.Id,
            entry.DirectoryFilter,
            entry.RelativeModelPath,
            entry.Flags,
            entry.RequestedAt,
            startedAt,
            completedAt,
            durationMs,
            outcome,
            error);
    }

    private static string SummarizeError(Exception ex)
    {
        var summary = ex.Message;
        if (string.IsNullOrWhiteSpace(summary))
            summary = ex.GetType().Name;
        return summary.Length <= 180 ? summary : summary[..180];
    }

    // -------------------------------------------------------------------------
    // Internal data class
    // -------------------------------------------------------------------------

    private sealed class IndexEntry(string? directoryFilter, string? relativeModelPath, IndexFlags flags)
    {
        public Guid Id { get; } = Guid.NewGuid();
        public string? DirectoryFilter { get; } = directoryFilter;
        public string? RelativeModelPath { get; } = relativeModelPath;
        public IndexFlags Flags { get; set; } = flags;
        public DateTime RequestedAt { get; } = DateTime.UtcNow;

        /// <summary>Back-reference to the linked-list node for O(1) removal.</summary>
        public LinkedListNode<IndexEntry>? Node { get; set; }

        public IndexRequestDto ToDto(string status) =>
            new(Id, DirectoryFilter, RelativeModelPath, Flags, RequestedAt, status);
    }

    private sealed class CompletedIndexEntry(
        Guid id,
        string? directoryFilter,
        string? relativeModelPath,
        IndexFlags flags,
        DateTime requestedAt,
        DateTime startedAt,
        DateTime completedAt,
        double durationMs,
        string outcome,
        string? error)
    {
        public Guid Id { get; } = id;
        public string? DirectoryFilter { get; } = directoryFilter;
        public string? RelativeModelPath { get; } = relativeModelPath;
        public IndexFlags Flags { get; } = flags;
        public DateTime RequestedAt { get; } = requestedAt;
        public DateTime StartedAt { get; } = startedAt;
        public DateTime CompletedAt { get; } = completedAt;
        public double DurationMs { get; } = durationMs;
        public string Outcome { get; } = outcome;
        public string? Error { get; } = error;

        public CompletedIndexRequestDto ToDto() =>
            new(
                Id,
                DirectoryFilter,
                RelativeModelPath,
                Flags,
                RequestedAt,
                StartedAt,
                CompletedAt,
                DurationMs,
                Outcome,
                Error);
    }
}
