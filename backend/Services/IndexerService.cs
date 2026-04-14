using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using System.Threading;

namespace findamodel.Services;

/// <summary>
/// Manages a prioritised queue of indexing requests and processes them one at a time
/// on a background task. Keeps all scanning logic inside ModelService and
/// MetadataConfigService; only orchestrates dispatch and sequencing.
/// </summary>
public class IndexerService(
    MetadataConfigService metadataConfigService,
    ModelService modelService,
    IDbContextFactory<ModelCacheContext> dbFactory,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Indexing);
    private readonly object _lock = new();
    private readonly LinkedList<IndexEntry> _queue = new();
    private readonly LinkedList<CompletedIndexEntry> _recent = new();
    private const int RecentLimit = 15;
    private static readonly TimeSpan HistoryRetention = TimeSpan.FromDays(7);
    private readonly SemaphoreSlim _singleRequestGate = new(1, 1);
    private IndexEntry? _current;
    private CancellationTokenSource? _currentCancellation;
    private Task? _processingTask;

    /// <summary>
    /// Adds an indexing request to the queue.
    /// If a request for the same target is already queued,
    /// its flags are merged and it is moved to the front of the queue.
    /// </summary>
    public IndexRequestDto Enqueue(string? directoryFilter, string? relativeModelPath, IndexFlags flags,
        Guid? existingRunId = null)
    {
        lock (_lock)
        {
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

            var entry = new IndexEntry(filter, modelPath, flags, existingRunId);
            entry.Node = _queue.AddLast(entry);

            if (_processingTask == null || _processingTask.IsCompleted)
                _processingTask = Task.Run(ProcessQueueAsync);

            return entry.ToDto("queued");
        }
    }

    /// <summary>Returns a point-in-time snapshot of the current queue state.</summary>
    /// <summary>
    /// Called once at startup: finds any IndexRun records left in "running" state
    /// (due to a previous server crash/restart), resets them to "queued" and
    /// re-enqueues them reusing the same RunId so the existing record transitions
    /// queued → running rather than creating a new record.
    /// </summary>
    public async Task RequeueInterruptedRunsAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var interrupted = await db.IndexRuns
            .Where(r => r.Status == "running")
            .OrderBy(r => r.RequestedAt)
            .ToListAsync();

        if (interrupted.Count == 0)
            return;

        // Remove stale partial progress data from the interrupted runs.
        var interruptedIds = interrupted.Select(r => r.Id).ToList();
        db.IndexRunFiles.RemoveRange(db.IndexRunFiles.Where(f => interruptedIds.Contains(f.IndexRunId)));
        db.IndexRunEvents.RemoveRange(db.IndexRunEvents.Where(e => interruptedIds.Contains(e.IndexRunId)));

        foreach (var run in interrupted)
        {
            run.Status = "queued";
            run.Outcome = null;
            run.Error = null;
            run.StartedAt = null;
            run.CompletedAt = null;
            run.ProcessedFiles = 0;
            run.TotalFiles = null;
        }

        await db.SaveChangesAsync();

        // Re-enqueue in original request order (FIFO), reusing the existing RunId.
        foreach (var run in interrupted)
        {
            logger.LogInformation(
                "Re-queueing interrupted index run {RunId} ({Flags}, filter: {Filter})",
                run.Id, (IndexFlags)run.Flags, run.DirectoryFilter ?? run.RelativeModelPath ?? "(all)");
            Enqueue(run.DirectoryFilter, run.RelativeModelPath, (IndexFlags)run.Flags, existingRunId: run.Id);
        }
    }

    public IndexerStatusDto GetStatus()
    {
        lock (_lock)
        {
            var currentDto = _current?.ToDto("running");
            var queueDtos = _queue
                .Select(e => e.ToDto("queued"))
                .ToList();
            var recentDtos = _recent.Select(e => e.ToDto()).ToList();
            return new IndexerStatusDto(_current != null, currentDto, queueDtos, recentDtos);
        }
    }

    public async Task<bool> CancelAsync(Guid requestOrRunId)
    {
        IndexEntry? queuedMatch = null;
        CancellationTokenSource? currentCancellation = null;

        lock (_lock)
        {
            queuedMatch = _queue.FirstOrDefault(e => MatchesRequest(e, requestOrRunId));
            if (queuedMatch is not null)
            {
                _queue.Remove(queuedMatch.Node!);
            }
            else if (_current is not null && MatchesRequest(_current, requestOrRunId))
            {
                currentCancellation = _currentCancellation;
            }
            else
            {
                return false;
            }
        }

        if (queuedMatch is not null)
        {
            await UpsertCancelledQueuedRunAsync(queuedMatch);
            return true;
        }

        currentCancellation?.Cancel();
        return true;
    }

    public async Task<IReadOnlyList<IndexRunSummaryDto>> GetHistoryAsync(int days = 7, int limit = 250)
    {
        var retentionCutoff = DateTime.UtcNow.AddDays(-Math.Max(1, days));
        await using var db = await dbFactory.CreateDbContextAsync();
        var runs = await db.IndexRuns
            .AsNoTracking()
            .Where(r => r.RequestedAt >= retentionCutoff)
            .OrderByDescending(r => r.RequestedAt)
            .Take(Math.Clamp(limit, 1, 1000))
            .ToListAsync();

        return runs.Select(ToSummaryDto).ToList();
    }

    public async Task<IndexRunDetailDto?> GetRunDetailAsync(
        Guid runId,
        int filesPage = 1,
        int filesPageSize = 200,
        string filesView = "all",
        int eventsPage = 1,
        int eventsPageSize = 200)
    {
        var normalizedFilesPage = Math.Max(1, filesPage);
        var normalizedFilesPageSize = Math.Clamp(filesPageSize, 25, 1000);
        var normalizedEventsPage = Math.Max(1, eventsPage);
        var normalizedEventsPageSize = Math.Clamp(eventsPageSize, 25, 1000);

        await using var db = await dbFactory.CreateDbContextAsync();
        var run = await db.IndexRuns
            .AsNoTracking()
            .FirstOrDefaultAsync(r => r.Id == runId);
        if (run == null)
            return null;

        var filesQuery = db.IndexRunFiles
            .AsNoTracking()
            .Where(f => f.IndexRunId == runId);

        var normalizedFilesView = (filesView ?? "all").Trim().ToLowerInvariant();
        if (normalizedFilesView == "pending")
            filesQuery = filesQuery.Where(f => f.Status == "pending");
        else if (normalizedFilesView == "processed")
            filesQuery = filesQuery.Where(f => f.Status != "pending");

        var filesTotalCount = await filesQuery.CountAsync();
        var orderedFilesQuery = normalizedFilesView switch
        {
            "pending" => filesQuery.OrderBy(f => f.RelativePath),
            "processed" => filesQuery
                .OrderByDescending(f => f.ProcessedAt)
                .ThenBy(f => f.RelativePath),
            _ => filesQuery
                .OrderBy(f => f.Status == "pending")
                .ThenByDescending(f => f.ProcessedAt)
                .ThenBy(f => f.RelativePath)
        };

        var files = await orderedFilesQuery
            .Skip((normalizedFilesPage - 1) * normalizedFilesPageSize)
            .Take(normalizedFilesPageSize)
            .Select(f => new IndexRunFileDto(
                f.RelativePath,
                f.FileType,
                f.Status,
                f.IsNew,
                f.WasUpdated,
                f.GeneratedPreview,
                f.GeneratedHull,
                f.GeneratedAiTags,
                f.GeneratedAiDescription,
                f.AiGenerationReason,
                f.Message,
                f.DurationMs,
                f.ProcessedAt))
            .ToListAsync();

        var eventsQuery = db.IndexRunEvents
            .AsNoTracking()
            .Where(e => e.IndexRunId == runId);
        var eventsTotalCount = await eventsQuery.CountAsync();
        var events = await eventsQuery
            .OrderByDescending(e => e.CreatedAt)
            .Skip((normalizedEventsPage - 1) * normalizedEventsPageSize)
            .Take(normalizedEventsPageSize)
            .Select(e => new IndexRunEventDto(e.CreatedAt, e.Level, e.Message, e.RelativePath))
            .ToListAsync();

        return new IndexRunDetailDto(
            ToSummaryDto(run),
            new PagedResultDto<IndexRunFileDto>(files, normalizedFilesPage, normalizedFilesPageSize, filesTotalCount),
            new PagedResultDto<IndexRunEventDto>(events, normalizedEventsPage, normalizedEventsPageSize, eventsTotalCount));
    }

    private IndexEntry? FindQueued(string? filter, string? modelPath)
    {
        foreach (var entry in _queue)
            if (entry.DirectoryFilter == filter && entry.RelativeModelPath == modelPath)
                return entry;
        return null;
    }

    private static bool MatchesRequest(IndexEntry entry, Guid requestOrRunId) =>
        entry.Id == requestOrRunId || entry.RunId == requestOrRunId;

    private async Task ProcessQueueAsync()
    {
        while (true)
        {
            IndexEntry entry;
            CancellationTokenSource requestCancellation;
            lock (_lock)
            {
                if (_queue.Count == 0)
                {
                    _current = null;
                    _currentCancellation?.Dispose();
                    _currentCancellation = null;
                    _processingTask = null;
                    return;
                }
                entry = _queue.First!.Value;
                _queue.RemoveFirst();
                _current = entry;
                requestCancellation = new CancellationTokenSource();
                _currentCancellation = requestCancellation;
            }

            await _singleRequestGate.WaitAsync();
            CompletedIndexEntry completed;
            try
            {
                completed = await ProcessRequestAsync(entry, requestCancellation.Token);
            }
            finally
            {
                _singleRequestGate.Release();
            }

            lock (_lock)
            {
                _current = null;
                _currentCancellation?.Dispose();
                _currentCancellation = null;
                _recent.AddFirst(completed);
                while (_recent.Count > RecentLimit)
                    _recent.RemoveLast();
            }
        }
    }

    private async Task<CompletedIndexEntry> ProcessRequestAsync(IndexEntry entry, CancellationToken cancellationToken)
    {
        var label = entry.RelativeModelPath is not null
            ? $"model:{entry.RelativeModelPath}"
            : entry.DirectoryFilter ?? "(all)";
        var startedAt = DateTime.UtcNow;
        var completedAt = startedAt;
        var outcome = "success";
        string? error = null;
        var runRecordCreated = false;

        var progressReporter = new RunProgressReporter(
            dbFactory,
            entry.RunId,
            (total, processed) =>
            {
                lock (_lock)
                {
                    entry.TotalFiles = total;
                    entry.ProcessedFiles = processed;
                }
            });

        try
        {
            await CreateRunAsync(entry, startedAt, cancellationToken);
            runRecordCreated = true;
            await CleanupOldRunsAsync(cancellationToken);

            logger.LogInformation("IndexerService: starting {Flags} for {Target}", entry.Flags, label);
            await progressReporter.OnLogAsync("info", $"Starting run for {label}");

            cancellationToken.ThrowIfCancellationRequested();

            if (entry.Flags.HasFlag(IndexFlags.Directories))
            {
                await progressReporter.OnLogAsync("info", "Syncing directory configs");
                await metadataConfigService.SyncDirectoryConfigsAsync();
                cancellationToken.ThrowIfCancellationRequested();
            }

            if (entry.Flags.HasFlag(IndexFlags.Models))
            {
                await progressReporter.OnLogAsync("info", "Scanning model files");
                if (entry.RelativeModelPath is not null)
                    await modelService.ScanAndCacheSingleAsync(entry.RelativeModelPath, progressReporter, cancellationToken);
                else
                    await modelService.ScanAndCacheAsync(
                        directoryFilter: entry.DirectoryFilter,
                        progressReporter: progressReporter,
                        cancellationToken: cancellationToken);
            }

            logger.LogInformation("IndexerService: completed {Flags} for {Target}", entry.Flags, label);
            await progressReporter.OnLogAsync("info", "Run completed successfully");
        }
        catch (OperationCanceledException)
        {
            outcome = "cancelled";
            error = "Cancelled by user.";
            logger.LogInformation("IndexerService: cancelled {Flags} for {Target}", entry.Flags, label);
            if (runRecordCreated)
                await progressReporter.OnLogAsync("info", "Run cancelled by user");
        }
        catch (Exception ex)
        {
            outcome = "failed";
            error = SummarizeError(ex);
            logger.LogError(ex, "IndexerService: failed processing {Flags} for {Target}", entry.Flags, label);
            if (runRecordCreated)
                await progressReporter.OnLogAsync("error", $"Run failed: {error}");
        }

        completedAt = DateTime.UtcNow;
        var durationMs = Math.Max(0, (completedAt - startedAt).TotalMilliseconds);

        if (!runRecordCreated && outcome == "cancelled")
        {
            await UpsertCancelledQueuedRunAsync(entry);
        }
        else
        {
            await CompleteRunAsync(entry.RunId, completedAt, durationMs, outcome, error, CancellationToken.None);
        }

        return new CompletedIndexEntry(
            entry.Id,
            entry.RunId,
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

    private async Task UpsertCancelledQueuedRunAsync(IndexEntry entry)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var now = DateTime.UtcNow;
        var run = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == entry.RunId);
        if (run is null)
        {
            db.IndexRuns.Add(new IndexRun
            {
                Id = entry.RunId,
                DirectoryFilter = entry.DirectoryFilter,
                RelativeModelPath = entry.RelativeModelPath,
                Flags = (int)entry.Flags,
                RequestedAt = entry.RequestedAt,
                CompletedAt = now,
                DurationMs = 0,
                Status = "cancelled",
                Outcome = "cancelled",
                Error = "Cancelled before start.",
                ProcessedFiles = 0,
            });
        }
        else
        {
            run.CompletedAt = now;
            run.DurationMs = 0;
            run.Status = "cancelled";
            run.Outcome = "cancelled";
            run.Error = "Cancelled before start.";
        }

        await db.SaveChangesAsync();
    }

    private async Task CreateRunAsync(IndexEntry entry, DateTime startedAt, CancellationToken cancellationToken)
    {
        await using var db = await dbFactory.CreateDbContextAsync(cancellationToken);
        var existingRun = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == entry.RunId, cancellationToken);
        if (existingRun != null)
        {
            // Re-queued after a server restart - update the existing record in-place.
            existingRun.StartedAt = startedAt;
            existingRun.Status = "running";
            existingRun.ProcessedFiles = 0;
            existingRun.TotalFiles = null;
            existingRun.CompletedAt = null;
            existingRun.Outcome = null;
            existingRun.Error = null;
        }
        else
        {
            db.IndexRuns.Add(new IndexRun
            {
                Id = entry.RunId,
                DirectoryFilter = entry.DirectoryFilter,
                RelativeModelPath = entry.RelativeModelPath,
                Flags = (int)entry.Flags,
                RequestedAt = entry.RequestedAt,
                StartedAt = startedAt,
                Status = "running",
                ProcessedFiles = 0,
            });
        }
        await db.SaveChangesAsync(cancellationToken);
    }

    private async Task CompleteRunAsync(
        Guid runId,
        DateTime completedAt,
        double durationMs,
        string outcome,
        string? error,
        CancellationToken cancellationToken)
    {
        await using var db = await dbFactory.CreateDbContextAsync(cancellationToken);
        var run = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == runId, cancellationToken);
        if (run == null)
            return;

        run.CompletedAt = completedAt;
        run.DurationMs = durationMs;
        run.Outcome = outcome;
        run.Error = error;
        run.Status = outcome;
        await db.SaveChangesAsync(cancellationToken);
    }

    private async Task CleanupOldRunsAsync(CancellationToken cancellationToken)
    {
        var cutoff = DateTime.UtcNow - HistoryRetention;
        await using var db = await dbFactory.CreateDbContextAsync(cancellationToken);
        var staleRunIds = await db.IndexRuns
            .AsNoTracking()
            .Where(r => r.RequestedAt < cutoff)
            .Select(r => r.Id)
            .ToListAsync(cancellationToken);

        if (staleRunIds.Count == 0)
            return;

        var staleFiles = db.IndexRunFiles.Where(f => staleRunIds.Contains(f.IndexRunId));
        var staleEvents = db.IndexRunEvents.Where(e => staleRunIds.Contains(e.IndexRunId));
        var staleRuns = db.IndexRuns.Where(r => staleRunIds.Contains(r.Id));

        db.IndexRunFiles.RemoveRange(staleFiles);
        db.IndexRunEvents.RemoveRange(staleEvents);
        db.IndexRuns.RemoveRange(staleRuns);
        await db.SaveChangesAsync(cancellationToken);
    }

    private static string SummarizeError(Exception ex)
    {
        var summary = ex.Message;
        if (string.IsNullOrWhiteSpace(summary))
            summary = ex.GetType().Name;
        return summary.Length <= 180 ? summary : summary[..180];
    }

    private static IndexRunSummaryDto ToSummaryDto(IndexRun run) =>
        new(
            run.Id,
            run.DirectoryFilter,
            run.RelativeModelPath,
            (IndexFlags)run.Flags,
            run.RequestedAt,
            run.StartedAt,
            run.CompletedAt,
            run.TotalFiles,
            run.ProcessedFiles,
            run.Status,
            run.Outcome,
            run.Error);

    private sealed class IndexEntry(string? directoryFilter, string? relativeModelPath, IndexFlags flags,
        Guid? runId = null)
    {
        public Guid Id { get; } = Guid.NewGuid();
        public Guid RunId { get; } = runId ?? Guid.NewGuid();
        public string? DirectoryFilter { get; } = directoryFilter;
        public string? RelativeModelPath { get; } = relativeModelPath;
        public IndexFlags Flags { get; set; } = flags;
        public DateTime RequestedAt { get; } = DateTime.UtcNow;
        public int? TotalFiles { get; set; }
        public int ProcessedFiles { get; set; }

        public LinkedListNode<IndexEntry>? Node { get; set; }

        public IndexRequestDto ToDto(string status) =>
            new(Id, RunId, DirectoryFilter, RelativeModelPath, Flags, RequestedAt, TotalFiles, ProcessedFiles, status);
    }

    private sealed class CompletedIndexEntry(
        Guid id,
        Guid runId,
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
        public Guid RunId { get; } = runId;
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
                RunId,
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

    private sealed class RunProgressReporter(
        IDbContextFactory<ModelCacheContext> dbFactory,
        Guid runId,
        Action<int?, int> setProgress) : IIndexingProgressReporter
    {
        private int? _totalFiles;
        private int _processedFiles;

        public async Task OnScanStartedAsync(int totalFiles)
        {
            _totalFiles = totalFiles;
            _processedFiles = 0;
            setProgress(_totalFiles, _processedFiles);

            await using var db = await dbFactory.CreateDbContextAsync();
            var run = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == runId);
            if (run == null)
                return;

            run.TotalFiles = totalFiles;
            run.ProcessedFiles = 0;
            await db.SaveChangesAsync();
        }

        public async Task OnFilesDiscoveredAsync(IReadOnlyList<IndexingFilePlan> files)
        {
            if (files.Count == 0)
                return;

            await using var db = await dbFactory.CreateDbContextAsync();
            var existing = await db.IndexRunFiles
                .Where(f => f.IndexRunId == runId)
                .Select(f => f.RelativePath)
                .ToListAsync();
            var existingSet = new HashSet<string>(existing, StringComparer.OrdinalIgnoreCase);

            var additions = files
                .Select(f => new IndexingFilePlan(NormalizeRelativePath(f.RelativePath), f.FileType))
                .Where(f => !existingSet.Contains(f.RelativePath))
                .Select(f => new IndexRunFile
                {
                    Id = Guid.NewGuid(),
                    IndexRunId = runId,
                    RelativePath = f.RelativePath,
                    FileType = f.FileType,
                    Status = "pending",
                })
                .ToList();

            if (additions.Count == 0)
                return;

            db.IndexRunFiles.AddRange(additions);
            await db.SaveChangesAsync();
        }

        public async Task OnFileProcessedAsync(IndexingFileResult fileResult)
        {
            var normalizedRelativePath = NormalizeRelativePath(fileResult.RelativePath);

            await using var db = await dbFactory.CreateDbContextAsync();
            var run = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == runId);
            if (run == null)
                return;

            var file = await db.IndexRunFiles
                .FirstOrDefaultAsync(f => f.IndexRunId == runId && f.RelativePath == normalizedRelativePath);

            var firstCompletionForFile = file?.ProcessedAt is null;

            if (file == null)
            {
                file = new IndexRunFile
                {
                    Id = Guid.NewGuid(),
                    IndexRunId = runId,
                    RelativePath = normalizedRelativePath,
                    FileType = fileResult.FileType,
                };
                db.IndexRunFiles.Add(file);
                firstCompletionForFile = true;
            }

            if (firstCompletionForFile)
                _processedFiles++;

            setProgress(_totalFiles, _processedFiles);

            file.Status = fileResult.Status;
            file.IsNew = fileResult.IsNew;
            file.WasUpdated = fileResult.WasUpdated;
            file.GeneratedPreview = fileResult.GeneratedPreview;
            file.GeneratedHull = fileResult.GeneratedHull;
            file.GeneratedAiTags = fileResult.GeneratedAiTags;
            file.GeneratedAiDescription = fileResult.GeneratedAiDescription;
            file.AiGenerationReason = fileResult.AiGenerationReason;
            file.Message = fileResult.Message;
            file.DurationMs = fileResult.DurationMs;
            file.ProcessedAt = DateTime.UtcNow;

            run.TotalFiles = _totalFiles;
            run.ProcessedFiles = _processedFiles;

            await db.SaveChangesAsync();
        }

        public async Task OnLogAsync(string level, string message, string? relativePath = null)
        {
            await using var db = await dbFactory.CreateDbContextAsync();
            db.IndexRunEvents.Add(new IndexRunEvent
            {
                Id = Guid.NewGuid(),
                IndexRunId = runId,
                CreatedAt = DateTime.UtcNow,
                Level = string.IsNullOrWhiteSpace(level) ? "info" : level,
                Message = message,
                RelativePath = relativePath is null ? null : NormalizeRelativePath(relativePath),
            });
            await db.SaveChangesAsync();
        }

        private static string NormalizeRelativePath(string relativePath) =>
            relativePath.Replace('\\', '/').TrimStart('/');
    }
}
