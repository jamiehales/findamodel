using System.Collections.Concurrent;
using System.IO.Compression;
using System.Text.RegularExpressions;
using findamodel.Data;
using findamodel.Models;
using Microsoft.EntityFrameworkCore;

namespace findamodel.Services;

public sealed class PrintingListArchiveService(
    IDbContextFactory<ModelCacheContext> dbFactory,
    IConfiguration config,
    ILogger<PrintingListArchiveService> logger)
{
    private static readonly TimeSpan JobRetention = TimeSpan.FromHours(1);
    private static readonly Regex InvalidFileNameChars = new($"[{Regex.Escape(new string(Path.GetInvalidFileNameChars()))}]+", RegexOptions.Compiled);
    private readonly ConcurrentDictionary<Guid, ArchiveJobState> jobs = new();

    public async Task<PrintingListArchiveJobDto?> CreateJobAsync(
        Guid listId,
        Guid userId,
        bool isAdmin,
        bool flatten = false,
        CancellationToken cancellationToken = default)
    {
        CleanupExpiredJobs();

        var modelsRootPath = config["Models:DirectoryPath"];
        if (string.IsNullOrWhiteSpace(modelsRootPath) || !Directory.Exists(modelsRootPath))
            throw new InvalidOperationException("Models directory is not configured.");

        await using var db = await dbFactory.CreateDbContextAsync(cancellationToken);

        var list = await db.PrintingLists
            .AsNoTracking()
            .FirstOrDefaultAsync(l => l.Id == listId, cancellationToken);

        if (list == null) return null;
        if (!isAdmin && list.OwnerId != userId) return null;

        var modelRows = await db.PrintingListItems
            .AsNoTracking()
            .Where(i => i.PrintingListId == listId && i.Quantity > 0)
            .Join(
                db.Models.AsNoTracking(),
                item => item.ModelId,
                model => model.Id,
                (item, model) => new
                {
                    model.Id,
                    model.Directory,
                    model.FileName,
                    item.Quantity,
                })
            .OrderBy(row => row.Directory)
            .ThenBy(row => row.FileName)
            .Select(row => new ArchiveSourceRow(
                row.Id,
                row.Directory,
                row.FileName,
                row.Quantity))
            .ToListAsync(cancellationToken);

        var archiveEntries = BuildArchiveEntries(modelsRootPath, modelRows, flatten);
        var tempDirectory = Path.Combine(Path.GetTempPath(), "findamodel", "printing-list-archives");
        Directory.CreateDirectory(tempDirectory);

        var fileName = BuildArchiveFileName(list.Name);
        var tempFilePath = Path.Combine(tempDirectory, $"{Guid.NewGuid():N}.zip");

        var job = new ArchiveJobState(Guid.NewGuid(), userId, fileName, tempFilePath, archiveEntries.Count);
        jobs[job.JobId] = job;

        _ = Task.Run(() => RunJobAsync(job, archiveEntries), CancellationToken.None);

        return job.ToDto();
    }

    public PrintingListArchiveJobDto? GetJob(Guid jobId, Guid userId, bool isAdmin)
    {
        CleanupExpiredJobs();

        if (!jobs.TryGetValue(jobId, out var job)) return null;
        if (!isAdmin && job.OwnerId != userId) return null;
        return job.ToDto();
    }

    public (string Path, string FileName)? GetCompletedJobFile(Guid jobId, Guid userId, bool isAdmin)
    {
        CleanupExpiredJobs();

        if (!jobs.TryGetValue(jobId, out var job)) return null;
        if (!isAdmin && job.OwnerId != userId) return null;
        if (!job.IsCompletedSuccessfully || !File.Exists(job.TempFilePath)) return null;

        return (job.TempFilePath, job.FileName);
    }

    public Task RemoveJobAsync(Guid jobId)
    {
        if (!jobs.TryRemove(jobId, out var job)) return Task.CompletedTask;

        try
        {
            if (File.Exists(job.TempFilePath))
                File.Delete(job.TempFilePath);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to delete archive temp file for job {JobId}", jobId);
        }

        return Task.CompletedTask;
    }

    private static List<ArchiveEntry> BuildArchiveEntries(
        string modelsRootPath,
        IReadOnlyList<ArchiveSourceRow> modelRows,
        bool flatten)
    {
        var entries = new List<ArchiveEntry>();

        var conflictingFlattenedFileNames = flatten
            ? modelRows
                .Where(row => !string.IsNullOrWhiteSpace(row.FileName) && row.Quantity > 0)
                .GroupBy(row => row.FileName, StringComparer.OrdinalIgnoreCase)
                .Where(group => group.Select(row => row.ModelId).Distinct().Count() > 1)
                .Select(group => group.Key)
                .ToHashSet(StringComparer.OrdinalIgnoreCase)
            : [];

        foreach (var row in modelRows)
        {
            var directory = row.Directory ?? string.Empty;
            var fileName = row.FileName ?? string.Empty;
            var quantity = row.Quantity;

            if (string.IsNullOrWhiteSpace(fileName) || quantity <= 0) continue;

            var fullSourcePath = string.IsNullOrEmpty(directory)
                ? Path.Combine(modelsRootPath, fileName)
                : Path.Combine(modelsRootPath, directory.Replace('/', Path.DirectorySeparatorChar), fileName);

            var extension = Path.GetExtension(fileName);
            var stem = Path.GetFileNameWithoutExtension(fileName);
            var appendModelId = flatten && conflictingFlattenedFileNames.Contains(fileName);
            var fileIdSuffix = appendModelId ? $"_{row.ModelId:N}" : string.Empty;

            string? zipDirectory = null;
            if (!flatten)
            {
                var baseRelativePath = string.IsNullOrEmpty(directory)
                    ? fileName
                    : $"{directory.Trim('/')}/{fileName}";
                zipDirectory = Path.GetDirectoryName(baseRelativePath.Replace('/', Path.DirectorySeparatorChar))?.Replace('\\', '/');
            }

            for (var instanceNumber = 1; instanceNumber <= quantity; instanceNumber++)
            {
                var instanceSuffix = quantity > 1 ? $"_{instanceNumber}" : string.Empty;
                var archiveName = $"{stem}{fileIdSuffix}{instanceSuffix}{extension}";
                var archivePath = string.IsNullOrEmpty(zipDirectory)
                    ? archiveName
                    : $"{zipDirectory}/{archiveName}";

                entries.Add(new ArchiveEntry(fullSourcePath, archivePath));
            }
        }

        return entries;
    }

    private async Task RunJobAsync(
        ArchiveJobState job,
        IReadOnlyList<ArchiveEntry> archiveEntries)
    {
        try
        {
            job.MarkRunning();

            await using var fileStream = new FileStream(
                job.TempFilePath,
                FileMode.Create,
                FileAccess.ReadWrite,
                FileShare.Read,
                bufferSize: 1024 * 64,
                useAsync: true);

            using (var archive = new ZipArchive(fileStream, ZipArchiveMode.Create, leaveOpen: true))
            {
                foreach (var archiveEntry in archiveEntries)
                {
                    if (!File.Exists(archiveEntry.SourcePath))
                        throw new FileNotFoundException($"Model file not found: {archiveEntry.SourcePath}", archiveEntry.SourcePath);

                    job.MarkCurrentEntry(archiveEntry.ArchivePath);

                    var entry = archive.CreateEntry(archiveEntry.ArchivePath, CompressionLevel.Fastest);
                    await using var entryStream = entry.Open();
                    await using var sourceStream = new FileStream(
                        archiveEntry.SourcePath,
                        FileMode.Open,
                        FileAccess.Read,
                        FileShare.Read,
                        bufferSize: 1024 * 64,
                        useAsync: true);

                    await sourceStream.CopyToAsync(entryStream);
                    job.MarkEntryCompleted();
                }
            }

            await fileStream.FlushAsync();
            job.MarkCompleted();
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to build printing list archive for job {JobId}", job.JobId);
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

    private static string BuildArchiveFileName(string listName)
    {
        var safeBaseName = InvalidFileNameChars.Replace(listName.Trim(), "-").Trim('-', ' ');
        if (string.IsNullOrWhiteSpace(safeBaseName)) safeBaseName = "printing-list";
        return $"{safeBaseName}-{DateTime.UtcNow:yyyyMMdd-HHmmss}.zip";
    }

    private sealed record ArchiveSourceRow(Guid ModelId, string Directory, string FileName, int Quantity);

    private sealed record ArchiveEntry(string SourcePath, string ArchivePath);

    private sealed class ArchiveJobState(
        Guid jobId,
        Guid ownerId,
        string fileName,
        string tempFilePath,
        int totalEntries)
    {
        private readonly object gate = new();
        private string status = "queued";
        private int completedEntries;
        private string? currentEntryName;
        private string? errorMessage;
        private DateTime updatedAtUtc = DateTime.UtcNow;

        public Guid JobId { get; } = jobId;
        public Guid OwnerId { get; } = ownerId;
        public string FileName { get; } = fileName;
        public string TempFilePath { get; } = tempFilePath;
        public int TotalEntries { get; } = totalEntries;

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

        public void MarkCurrentEntry(string archivePath)
        {
            lock (gate)
            {
                currentEntryName = archivePath;
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

        public void MarkCompleted()
        {
            lock (gate)
            {
                completedEntries = TotalEntries;
                currentEntryName = null;
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

        public PrintingListArchiveJobDto ToDto()
        {
            lock (gate)
            {
                var progressPercent = TotalEntries <= 0
                    ? (status == "completed" ? 100 : 0)
                    : (int)Math.Round(completedEntries * 100d / TotalEntries, MidpointRounding.AwayFromZero);

                return new PrintingListArchiveJobDto(
                    JobId,
                    FileName,
                    status,
                    TotalEntries,
                    completedEntries,
                    Math.Clamp(progressPercent, 0, 100),
                    currentEntryName,
                    errorMessage);
            }
        }
    }
}