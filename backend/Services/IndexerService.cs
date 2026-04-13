using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
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
    IDbContextFactory<ModelCacheContext> dbFactory,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Indexing);
    private readonly object _lock = new();
    private readonly LinkedList<IndexEntry> _queue = new();
    private readonly LinkedList<CompletedIndexEntry> _recent = new();
    private const int RecentLimit = 15;
    private static readonly TimeSpan HistoryRetention = TimeSpan.FromDays(7);
    private IndexEntry? _current;
    private Task? _processingTask;

    /// <summary>
    /// Adds an indexing request to the queue.
    /// If a request for the same target is already queued,
    /// its flags are merged and it is moved to the front of the queue.
    /// </summary>
    public IndexRequestDto Enqueue(string? directoryFilter, string? relativeModelPath, IndexFlags flags)
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
        var filesTotalCount = await filesQuery.CountAsync();
        var files = await filesQuery
            .OrderBy(f => f.RelativePath)
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

        await CleanupOldRunsAsync();
        await CreateRunAsync(entry, startedAt);

        logger.LogInformation("IndexerService: starting {Flags} for {Target}", entry.Flags, label);

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

        await progressReporter.OnLogAsync("info", $"Starting run for {label}");

        try
        {
            if (entry.Flags.HasFlag(IndexFlags.Directories))
            {
                await progressReporter.OnLogAsync("info", "Syncing directory configs");
                await metadataConfigService.SyncDirectoryConfigsAsync();
            }

            if (entry.Flags.HasFlag(IndexFlags.Models))
            {
                await progressReporter.OnLogAsync("info", "Scanning model files");
                if (entry.RelativeModelPath is not null)
                    await modelService.ScanAndCacheSingleAsync(entry.RelativeModelPath, progressReporter);
                else
                    await modelService.ScanAndCacheAsync(directoryFilter: entry.DirectoryFilter, progressReporter: progressReporter);
            }

            logger.LogInformation("IndexerService: completed {Flags} for {Target}", entry.Flags, label);
            await progressReporter.OnLogAsync("info", "Run completed successfully");
        }
        catch (Exception ex)
        {
            outcome = "failed";
            error = SummarizeError(ex);
            logger.LogError(ex, "IndexerService: failed processing {Flags} for {Target}", entry.Flags, label);
            await progressReporter.OnLogAsync("error", $"Run failed: {error}");
        }

        completedAt = DateTime.UtcNow;
        var durationMs = Math.Max(0, (completedAt - startedAt).TotalMilliseconds);

        await CompleteRunAsync(entry.RunId, completedAt, durationMs, outcome, error);

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

    private async Task CreateRunAsync(IndexEntry entry, DateTime startedAt)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
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
        await db.SaveChangesAsync();
    }

    private async Task CompleteRunAsync(
        Guid runId,
        DateTime completedAt,
        double durationMs,
        string outcome,
        string? error)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var run = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == runId);
        if (run == null)
            return;

        run.CompletedAt = completedAt;
        run.DurationMs = durationMs;
        run.Outcome = outcome;
        run.Error = error;
        run.Status = outcome;
        await db.SaveChangesAsync();
    }

    private async Task CleanupOldRunsAsync()
    {
        var cutoff = DateTime.UtcNow - HistoryRetention;
        await using var db = await dbFactory.CreateDbContextAsync();
        var staleRunIds = await db.IndexRuns
            .AsNoTracking()
            .Where(r => r.RequestedAt < cutoff)
            .Select(r => r.Id)
            .ToListAsync();

        if (staleRunIds.Count == 0)
            return;

        var staleFiles = db.IndexRunFiles.Where(f => staleRunIds.Contains(f.IndexRunId));
        var staleEvents = db.IndexRunEvents.Where(e => staleRunIds.Contains(e.IndexRunId));
        var staleRuns = db.IndexRuns.Where(r => staleRunIds.Contains(r.Id));

        db.IndexRunFiles.RemoveRange(staleFiles);
        db.IndexRunEvents.RemoveRange(staleEvents);
        db.IndexRuns.RemoveRange(staleRuns);
        await db.SaveChangesAsync();
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

    private sealed class IndexEntry(string? directoryFilter, string? relativeModelPath, IndexFlags flags)
    {
        public Guid Id { get; } = Guid.NewGuid();
        public Guid RunId { get; } = Guid.NewGuid();
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
