using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class ModelService(
    IConfiguration config,
    ILogger<ModelService> logger,
    IDbContextFactory<ModelCacheContext> dbFactory,
    ModelLoaderService loaderService,
    ModelPreviewService previewService,
    HullCalculationService hullCalculationService,
    MetadataConfigService metadataConfigService)
{
    private static readonly string[] ModelExtensions = [".stl", ".obj"];

    public async Task<List<ModelDto>> GetModelsAsync(int? limit = null)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        // Include DirectoryConfig to avoid N+1 queries
        var modelsQuery = db.Models
            .Include(m => m.DirectoryConfig)
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName);

        var models = await (limit.HasValue ? modelsQuery.Take(limit.Value) : modelsQuery).ToListAsync();

        return models.Select(m => new ModelDto
        {
            Id = m.Id,
            Name = Path.GetFileNameWithoutExtension(m.FileName),
            RelativePath = ComputeRelativePath(m.Directory, m.FileName),
            FileType = m.FileType,
            FileSize = m.FileSize,
            FileUrl = $"/api/models/{m.Id}/file",
            HasPreview = m.PreviewImagePath != null,
            PreviewUrl = m.PreviewImagePath != null ? $"/api/models/{m.Id}/preview" : null,
            Creator = m.DirectoryConfig?.Creator,
            Collection = m.DirectoryConfig?.Collection,
            Subcollection = m.DirectoryConfig?.Subcollection,
            Category = m.DirectoryConfig?.Category,
            Type = m.DirectoryConfig?.Type,
            Supported = m.DirectoryConfig?.Supported,
            ConvexHull = m.ConvexHullCoordinates,
            ConcaveHull = m.ConcaveHullCoordinates,
            DimensionXMm  = m.DimensionXMm,
            DimensionYMm  = m.DimensionYMm,
            DimensionZMm  = m.DimensionZMm,
            SphereCentreX = m.SphereCentreX,
            SphereCentreY = m.SphereCentreY,
            SphereCentreZ = m.SphereCentreZ,
            SphereRadius  = m.SphereRadius
        }).ToList();
    }

    public async Task<CachedModel?> GetModelAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        return await db.Models.FirstOrDefaultAsync(m => m.Id == id);
    }

    public async Task<ModelDto?> GetModelDtoAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var m = await db.Models
            .Where(m => m.Id == id)
            .Select(m => new
            {
                m.Id,
                m.FileName,
                m.Directory,
                m.FileType,
                m.FileSize,
                HasPreview = m.PreviewImagePath != null,
                Creator        = m.DirectoryConfig != null ? m.DirectoryConfig.Creator        : null,
                Collection    = m.DirectoryConfig != null ? m.DirectoryConfig.Collection    : null,
                Subcollection = m.DirectoryConfig != null ? m.DirectoryConfig.Subcollection : null,
                Category      = m.DirectoryConfig != null ? m.DirectoryConfig.Category      : null,
                Type          = m.DirectoryConfig != null ? m.DirectoryConfig.Type          : null,
                Supported     = m.DirectoryConfig != null ? m.DirectoryConfig.Supported     : null,
                m.ConvexHullCoordinates,
                m.ConcaveHullCoordinates,
                m.DimensionXMm,
                m.DimensionYMm,
                m.DimensionZMm,
                m.SphereCentreX,
                m.SphereCentreY,
                m.SphereCentreZ,
                m.SphereRadius
            })
            .FirstOrDefaultAsync();

        if (m == null) return null;

        return new ModelDto
        {
            Id            = m.Id,
            Name          = Path.GetFileNameWithoutExtension(m.FileName),
            RelativePath  = ComputeRelativePath(m.Directory, m.FileName),
            FileType      = m.FileType,
            FileSize      = m.FileSize,
            FileUrl       = $"/api/models/{m.Id}/file",
            HasPreview    = m.HasPreview,
            PreviewUrl    = m.HasPreview ? $"/api/models/{m.Id}/preview" : null,
            Creator        = m.Creator,
            Collection    = m.Collection,
            Subcollection = m.Subcollection,
            Category      = m.Category,
            Type          = m.Type,
            Supported     = m.Supported,
            ConvexHull    = m.ConvexHullCoordinates,
            ConcaveHull   = m.ConcaveHullCoordinates,
            DimensionXMm  = m.DimensionXMm,
            DimensionYMm  = m.DimensionYMm,
            DimensionZMm  = m.DimensionZMm,
            SphereCentreX = m.SphereCentreX,
            SphereCentreY = m.SphereCentreY,
            SphereCentreZ = m.SphereCentreZ,
            SphereRadius  = m.SphereRadius,
        };
    }

    public async Task<string?> GetPreviewImagePathAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        return await db.Models
            .Where(m => m.Id == id)
            .Select(m => m.PreviewImagePath)
            .FirstOrDefaultAsync();
    }

    public async Task ScanAndCacheAsync()
    {
        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath))
        {
            logger.LogWarning("Models:DirectoryPath is not configured");
            return;
        }

        if (!Directory.Exists(modelsPath))
        {
            logger.LogWarning("Models directory not accessible: {Path}", modelsPath);
            return;
        }

        // Phase 1: Discover all model files
        var files = Directory.EnumerateFiles(modelsPath, "*.*", SearchOption.AllDirectories)
            .Where(f => ModelExtensions.Contains(Path.GetExtension(f).ToLower()))
            .ToList();

        var relativeDirectories = files
            .Select(f => (Path.GetDirectoryName(Path.GetRelativePath(modelsPath, f)) ?? "").Replace('\\', '/'))
            .Distinct(StringComparer.Ordinal)
            .ToList();

        // Phase 2: Sync DirectoryConfig records (root-first, hash-detected changes)
        var directoryConfigs = await metadataConfigService.EnsureDirectoryConfigsAsync(modelsPath, relativeDirectories);

        // Phase 3: Process new model files
        await using var db = await dbFactory.CreateDbContextAsync();
        var existingChecksums = (await db.Models.Select(m => m.Checksum).ToListAsync()).ToHashSet();

        var newCount = 0;
        foreach (var file in files)
        {
            var checksum = await ComputeChecksumAsync(file);
            if (existingChecksums.Contains(checksum)) continue;

            var relativePath = Path.GetRelativePath(modelsPath, file);
            var fileName = Path.GetFileName(file);
            var directory = (Path.GetDirectoryName(relativePath) ?? "").Replace('\\', '/');
            var info = new FileInfo(file);
            var fileType = Path.GetExtension(file).TrimStart('.').ToLower();

            logger.LogInformation("Discovered {FileType} model: {FilePath}", fileType.ToUpper(), file);

            // Load geometry once — parsed, transformed, and centred
            var geometry = await loaderService.LoadModelAsync(file, fileType);

            // Generate preview and hulls from pre-loaded geometry to avoid re-parsing
            var previewImagePath = geometry is not null
                ? await previewService.GeneratePreviewAsync(geometry, checksum)
                : null;
            var (convexHull, concaveHull) = geometry is not null
                ? await hullCalculationService.CalculateHullsAsync(geometry)
                : (null, null);

            directoryConfigs.TryGetValue(directory, out var dirConfig);

            var entity = new CachedModel
            {
                Id = Guid.NewGuid(),
                Checksum = checksum,
                FileName = fileName,
                Directory = directory,
                FileType = fileType,
                FileSize = info.Length,
                FileModifiedAt = info.LastWriteTimeUtc,
                CachedAt = DateTime.UtcNow,
                PreviewImagePath = previewImagePath,
                PreviewGeneratedAt = previewImagePath != null ? DateTime.UtcNow : null,
                ConvexHullCoordinates = convexHull,
                ConcaveHullCoordinates = concaveHull,
                HullGeneratedAt = convexHull != null || concaveHull != null ? DateTime.UtcNow : null,
                DirectoryConfigId = dirConfig?.Id,
                DimensionXMm        = geometry?.DimensionXMm,
                DimensionYMm        = geometry?.DimensionYMm,
                DimensionZMm        = geometry?.DimensionZMm,
                SphereCentreX       = geometry?.SphereCentre.X,
                SphereCentreY       = geometry?.SphereCentre.Y,
                SphereCentreZ       = geometry?.SphereCentre.Z,
                SphereRadius        = geometry?.SphereRadius,
                GeometryCalculatedAt = geometry is not null ? DateTime.UtcNow : null
            };

            db.Models.Add(entity);
            await db.SaveChangesAsync();
            existingChecksums.Add(checksum);
            newCount++;
        }

        logger.LogInformation("Scan complete: {New} new models added, {Total} total in cache.", newCount, await db.Models.CountAsync());
    }

    private static async Task<string> ComputeChecksumAsync(string filePath)
    {
        using var stream = File.OpenRead(filePath);
        var hash = await System.Security.Cryptography.SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLower();
    }

    private static string ComputeRelativePath(string directory, string fileName) =>
        string.IsNullOrEmpty(directory) ? fileName : $"{directory}/{fileName}";
}
