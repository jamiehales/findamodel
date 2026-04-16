using System.Collections.Concurrent;
using findamodel.Models;

namespace findamodel.Services;

public sealed class PlateGenerationJobService(
    PlateExportService plateExportService,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.PrintingList);
    private static readonly TimeSpan JobRetention = TimeSpan.FromHours(1);
    private readonly ConcurrentDictionary<Guid, PlateJobState> jobs = new();

    public Task<PlateGenerationJobDto> CreateJobAsync(
        GeneratePlateRequest request,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        CleanupExpiredJobs();

        var format = PlateExportService.NormalizeFormat(request.Format);
        var tempDirectory = Path.Combine(Path.GetTempPath(), "findamodel", "plate-exports");
        Directory.CreateDirectory(tempDirectory);

        var job = new PlateJobState(
            Guid.NewGuid(),
            PlateExportService.GetFileName(format),
            format,
            Path.Combine(tempDirectory, $"{Guid.NewGuid():N}.{format}"),
            request.Placements.Count);

        jobs[job.JobId] = job;
        _ = Task.Run(() => RunJobAsync(job, request), CancellationToken.None);

        return Task.FromResult(job.ToDto());
    }

    public PlateGenerationJobDto? GetJob(Guid jobId)
    {
        CleanupExpiredJobs();
        return jobs.TryGetValue(jobId, out var job) ? job.ToDto() : null;
    }

    public (string Path, string FileName, string? Warning, IReadOnlyList<string> SkippedModels)? GetCompletedJobFile(Guid jobId)
    {
        CleanupExpiredJobs();

        if (!jobs.TryGetValue(jobId, out var job)) return null;
        if (!job.IsCompletedSuccessfully || !File.Exists(job.TempFilePath)) return null;

        return (job.TempFilePath, job.FileName, job.Warning, job.SkippedModels);
    }

    public Task RemoveJobAsync(Guid jobId)
    {
        if (!jobs.TryRemove(jobId, out var job)) return Task.CompletedTask;

        DeleteTempFileIfPresent(job.TempFilePath);
        return Task.CompletedTask;
    }

    private async Task RunJobAsync(PlateJobState job, GeneratePlateRequest request)
    {
        try
        {
            job.MarkRunning();
            var result = await plateExportService.GeneratePlateAsync(request, job, CancellationToken.None);
            await File.WriteAllBytesAsync(job.TempFilePath, result.Content, CancellationToken.None);
            job.MarkCompleted(result.Warning, result.SkippedModels);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to build plate export for job {JobId}", job.JobId);
            job.MarkFailed(ex.Message);
            DeleteTempFileIfPresent(job.TempFilePath);
        }
    }

    private void CleanupExpiredJobs()
    {
        var expiredJobIds = jobs
            .Where(pair => pair.Value.IsExpired(JobRetention))
            .Select(pair => pair.Key)
            .ToList();

        foreach (var expiredJobId in expiredJobIds)
            RemoveJobAsync(expiredJobId).GetAwaiter().GetResult();
    }

    private static void DeleteTempFileIfPresent(string tempFilePath)
    {
        try
        {
            if (File.Exists(tempFilePath))
                File.Delete(tempFilePath);
        }
        catch { }
    }

    private sealed class PlateJobState(
        Guid jobId,
        string fileName,
        string format,
        string tempFilePath,
        int totalEntries) : IPlateGenerationProgressReporter
    {
        private readonly object gate = new();
        private string status = "queued";
        private int completedEntries;
        private string? currentEntryName;
        private string? errorMessage;
        private string? warning;
        private IReadOnlyList<string> skippedModels = [];
        private DateTime updatedAtUtc = DateTime.UtcNow;

        public Guid JobId { get; } = jobId;
        public string FileName { get; } = fileName;
        public string Format { get; } = format;
        public string TempFilePath { get; } = tempFilePath;
        public int TotalEntries { get; } = totalEntries;

        public string? Warning
        {
            get
            {
                lock (gate)
                    return warning;
            }
        }

        public IReadOnlyList<string> SkippedModels
        {
            get
            {
                lock (gate)
                    return skippedModels;
            }
        }

        public bool IsCompletedSuccessfully
        {
            get
            {
                lock (gate)
                    return status == "completed";
            }
        }

        public void MarkRunning()
        {
            lock (gate)
            {
                status = "running";
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public void MarkCurrentEntry(string entryName)
        {
            lock (gate)
            {
                currentEntryName = entryName;
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public void MarkEntryCompleted()
        {
            lock (gate)
            {
                completedEntries++;
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public void MarkCompleted(string? nextWarning, IReadOnlyList<string> nextSkippedModels)
        {
            lock (gate)
            {
                completedEntries = TotalEntries;
                currentEntryName = null;
                warning = nextWarning;
                skippedModels = nextSkippedModels;
                status = "completed";
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public void MarkFailed(string message)
        {
            lock (gate)
            {
                status = "failed";
                errorMessage = message;
                currentEntryName = null;
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public bool IsExpired(TimeSpan retention)
        {
            lock (gate)
                return DateTime.UtcNow - updatedAtUtc > retention && status is "completed" or "failed";
        }

        public PlateGenerationJobDto ToDto()
        {
            lock (gate)
            {
                var progressPercent = TotalEntries <= 0
                    ? (status == "completed" ? 100 : 0)
                    : (int)Math.Round(completedEntries * 100d / TotalEntries, MidpointRounding.AwayFromZero);

                return new PlateGenerationJobDto(
                    JobId,
                    FileName,
                    Format,
                    status,
                    TotalEntries,
                    completedEntries,
                    Math.Clamp(progressPercent, 0, 100),
                    currentEntryName,
                    errorMessage,
                    warning,
                    skippedModels);
            }
        }
    }
}
