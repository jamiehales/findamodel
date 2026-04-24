using System.Buffers.Binary;
using System.Collections.Concurrent;
using findamodel.Models;

namespace findamodel.Services;

public sealed class AutoSupportJobService(
    ModelService modelService,
    ModelLoaderService loaderService,
    MeshTransferService meshTransferService,
    AutoSupportGenerationService autoSupportGenerationService,
    IConfiguration config,
    ILoggerFactory loggerFactory)
{
    private static readonly TimeSpan JobRetention = TimeSpan.FromHours(1);
    private readonly ILogger logger = loggerFactory.CreateLogger<AutoSupportJobService>();
    private readonly ConcurrentDictionary<Guid, AutoSupportJobState> jobs = new();
    private readonly string cacheDirectory = config["Cache:AutoSupportsPath"]
        ?? Path.Combine(Path.GetTempPath(), "findamodel", "auto-support");

    public async Task<AutoSupportJobDto?> CreateJobAsync(Guid modelId, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        CleanupExpiredJobs();

        var model = await modelService.GetModelAsync(modelId);
        if (model == null)
            return null;

        Directory.CreateDirectory(cacheDirectory);
        var job = new AutoSupportJobState(
            Guid.NewGuid(),
            modelId,
            $"{Path.GetFileNameWithoutExtension(model.FileName)}-supported-preview.bin",
            Path.Combine(cacheDirectory, $"{Guid.NewGuid():N}.bin"));

        jobs[job.JobId] = job;
        _ = Task.Run(() => RunJobAsync(job), CancellationToken.None);
        return job.ToDto();
    }

    public AutoSupportJobDto? GetJob(Guid modelId, Guid jobId)
    {
        CleanupExpiredJobs();
        return jobs.TryGetValue(jobId, out var job) && job.ModelId == modelId
            ? job.ToDto()
            : null;
    }

    public byte[]? GetCompletedEnvelope(Guid modelId, Guid jobId)
    {
        CleanupExpiredJobs();

        if (!jobs.TryGetValue(jobId, out var job) || job.ModelId != modelId)
            return null;

        if (!job.IsCompletedSuccessfully || !File.Exists(job.CacheFilePath))
            return null;

        return File.ReadAllBytes(job.CacheFilePath);
    }

    private async Task RunJobAsync(AutoSupportJobState job)
    {
        try
        {
            job.MarkRunning();

            var model = await modelService.GetModelAsync(job.ModelId);
            if (model == null)
            {
                job.MarkFailed("Model not found.");
                return;
            }

            var modelsPath = config["Models:DirectoryPath"];
            if (string.IsNullOrWhiteSpace(modelsPath))
            {
                job.MarkFailed("Model directory is not configured.");
                return;
            }

            var fullPath = string.IsNullOrEmpty(model.Directory)
                ? Path.Combine(modelsPath, model.FileName)
                : Path.Combine(modelsPath, model.Directory, model.FileName);

            if (!File.Exists(fullPath))
            {
                job.MarkFailed("Model file was not found on disk.");
                return;
            }

            var geometry = await loaderService.LoadModelAsync(fullPath, model.FileType);
            if (geometry == null)
            {
                job.MarkFailed("Failed to load model geometry.");
                return;
            }

            var preview = autoSupportGenerationService.GenerateSupportPreview(geometry);
            var bodyPayload = meshTransferService.Encode(preview.BodyGeometry ?? geometry);
            var supportPayload = meshTransferService.Encode(preview.SupportGeometry);
            var envelope = BuildEnvelope(bodyPayload, supportPayload);
            await File.WriteAllBytesAsync(job.CacheFilePath, envelope, CancellationToken.None);

            job.MarkCompleted(
                preview.SupportPoints.Count,
                [.. preview.SupportPoints.Select(point => new AutoSupportPointDto(
                    point.Position.X,
                    point.Position.Y,
                    point.Position.Z,
                    point.RadiusMm,
                    new AutoSupportVectorDto(point.PullForce.X, point.PullForce.Y, point.PullForce.Z),
                    point.Size.ToString().ToLowerInvariant(),
                    point.LayerForces == null
                        ? null
                        : [.. point.LayerForces.Select(layer => new AutoSupportLayerForceDto(
                            layer.LayerIndex,
                            layer.SliceHeightMm,
                            new AutoSupportVectorDto(layer.Gravity.X, layer.Gravity.Y, layer.Gravity.Z),
                            new AutoSupportVectorDto(layer.Peel.X, layer.Peel.Y, layer.Peel.Z),
                            new AutoSupportVectorDto(layer.Rotation.X, layer.Rotation.Y, layer.Rotation.Z),
                            new AutoSupportVectorDto(layer.Total.X, layer.Total.Y, layer.Total.Z)))]))],
                [.. preview.Islands.Select(island => new AutoSupportIslandDto(
                    island.CentroidX,
                    island.CentroidZ,
                    island.SliceHeightMm,
                    island.AreaMm2,
                    island.RadiusMm,
                    island.Boundary == null
                        ? null
                        : [.. island.Boundary.Select(vertex => new AutoSupportVectorDto(vertex.X, 0f, vertex.Z))]))],
                preview.SliceLayers == null
                    ? []
                    : [.. preview.SliceLayers.Select(layer => new AutoSupportSliceLayerDto(
                        layer.LayerIndex,
                        layer.SliceHeightMm,
                        [.. layer.Islands.Select(island => new AutoSupportIslandDto(
                            island.CentroidX,
                            island.CentroidZ,
                            island.SliceHeightMm,
                            island.AreaMm2,
                            island.RadiusMm,
                            island.Boundary == null
                                ? null
                                : [.. island.Boundary.Select(vertex => new AutoSupportVectorDto(vertex.X, 0f, vertex.Z))]))],
                        layer.BedWidthMm,
                        layer.BedDepthMm,
                        layer.SliceMaskPngBase64))]);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to generate auto-support preview for job {JobId}", job.JobId);
            job.MarkFailed(ex.Message);
            DeleteFileIfPresent(job.CacheFilePath);
        }
    }

    private void CleanupExpiredJobs()
    {
        var expired = jobs
            .Where(pair => pair.Value.IsExpired(JobRetention))
            .Select(pair => pair.Key)
            .ToList();

        foreach (var jobId in expired)
        {
            if (!jobs.TryRemove(jobId, out var job))
                continue;

            DeleteFileIfPresent(job.CacheFilePath);
        }
    }

    private static byte[] BuildEnvelope(byte[] bodyPayload, byte[] supportPayload)
    {
        var envelope = new byte[4 + bodyPayload.Length + 4 + supportPayload.Length];
        var span = envelope.AsSpan();
        BinaryPrimitives.WriteUInt32LittleEndian(span[0..4], (uint)bodyPayload.Length);
        bodyPayload.CopyTo(span[4..]);
        var afterBody = 4 + bodyPayload.Length;
        BinaryPrimitives.WriteUInt32LittleEndian(span[afterBody..(afterBody + 4)], (uint)supportPayload.Length);
        supportPayload.CopyTo(span[(afterBody + 4)..]);
        return envelope;
    }

    private static void DeleteFileIfPresent(string path)
    {
        try
        {
            if (File.Exists(path))
                File.Delete(path);
        }
        catch
        {
        }
    }

    private sealed class AutoSupportJobState(Guid jobId, Guid modelId, string fileName, string cacheFilePath)
    {
        private readonly object gate = new();
        private string status = "queued";
        private int progressPercent;
        private int supportCount;
        private string? errorMessage;
        private IReadOnlyList<AutoSupportPointDto> supportPoints = [];
        private IReadOnlyList<AutoSupportIslandDto> islands = [];
        private IReadOnlyList<AutoSupportSliceLayerDto> sliceLayers = [];
        private DateTime updatedAtUtc = DateTime.UtcNow;

        public Guid JobId { get; } = jobId;
        public Guid ModelId { get; } = modelId;
        public string FileName { get; } = fileName;
        public string CacheFilePath { get; } = cacheFilePath;

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
                progressPercent = 25;
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public void MarkCompleted(
            int generatedSupportCount,
            IReadOnlyList<AutoSupportPointDto> generatedSupportPoints,
            IReadOnlyList<AutoSupportIslandDto> generatedIslands,
            IReadOnlyList<AutoSupportSliceLayerDto> generatedSliceLayers)
        {
            lock (gate)
            {
                status = "completed";
                progressPercent = 100;
                supportCount = generatedSupportCount;
                supportPoints = generatedSupportPoints;
                islands = generatedIslands;
                sliceLayers = generatedSliceLayers;
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public void MarkFailed(string message)
        {
            lock (gate)
            {
                status = "failed";
                progressPercent = 100;
                errorMessage = message;
                supportPoints = [];
                islands = [];
                sliceLayers = [];
                updatedAtUtc = DateTime.UtcNow;
            }
        }

        public bool IsExpired(TimeSpan retention)
        {
            lock (gate)
                return DateTime.UtcNow - updatedAtUtc > retention && status is "completed" or "failed";
        }

        public AutoSupportJobDto ToDto()
        {
            lock (gate)
                return new AutoSupportJobDto(JobId, status, progressPercent, supportCount, errorMessage, supportPoints, islands, sliceLayers);
        }
    }
}
