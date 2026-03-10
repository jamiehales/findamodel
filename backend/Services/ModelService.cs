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
        var modelsQuery = db.Models
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
            Creator = m.CalculatedCreator,
            Collection = m.CalculatedCollection,
            Subcollection = m.CalculatedSubcollection,
            Category = m.CalculatedCategory,
            Type = m.CalculatedType,
            Supported = m.CalculatedSupported,
            ConvexHull = m.ConvexHullCoordinates,
            ConcaveHull = m.ConcaveHullCoordinates,
            ConvexSansRaftHull = m.ConvexSansRaftHullCoordinates,
            RaftOffsetMm = HullCalculationService.RaftOffset,
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
                m.CalculatedCreator,
                m.CalculatedCollection,
                m.CalculatedSubcollection,
                m.CalculatedCategory,
                m.CalculatedType,
                m.CalculatedSupported,
                m.ConvexHullCoordinates,
                m.ConcaveHullCoordinates,
                m.ConvexSansRaftHullCoordinates,
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
            Creator        = m.CalculatedCreator,
            Collection    = m.CalculatedCollection,
            Subcollection = m.CalculatedSubcollection,
            Category      = m.CalculatedCategory,
            Type          = m.CalculatedType,
            Supported     = m.CalculatedSupported,
            ConvexHull         = m.ConvexHullCoordinates,
            ConcaveHull        = m.ConcaveHullCoordinates,
            ConvexSansRaftHull = m.ConvexSansRaftHullCoordinates,
            RaftOffsetMm       = HullCalculationService.RaftOffset,
            DimensionXMm       = m.DimensionXMm,
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

    public async Task<int> ScanAndCacheAsync(int? limit = null, string? directoryFilter = null)
    {
        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath))
        {
            logger.LogWarning("Models:DirectoryPath is not configured");
            return 0;
        }

        if (!Directory.Exists(modelsPath))
        {
            logger.LogWarning("Models directory not accessible: {Path}", modelsPath);
            return 0;
        }

        // Phase 1: Discover model files (scoped to directoryFilter when set)
        var searchRoot = directoryFilter is not null
            ? Path.Combine(modelsPath, directoryFilter.Replace('/', Path.DirectorySeparatorChar))
            : modelsPath;

        if (!Directory.Exists(searchRoot))
        {
            logger.LogWarning("Filter directory not found: {Path}", searchRoot);
            return 0;
        }

        var files = Directory.EnumerateFiles(searchRoot, "*.*", SearchOption.AllDirectories)
            .Where(f => ModelExtensions.Contains(Path.GetExtension(f).ToLower()))
            .ToList();

        var relativeDirectories = files
            .Select(f => (Path.GetDirectoryName(Path.GetRelativePath(modelsPath, f)) ?? "").Replace('\\', '/'))
            .Distinct(StringComparer.Ordinal)
            .ToList();

        // Phase 2: Sync DirectoryConfig records (root-first, hash-detected changes)
        var directoryConfigs = await metadataConfigService.EnsureDirectoryConfigsAsync(modelsPath, relativeDirectories);

        // Phase 3: Process new and updated model files
        await using var db = await dbFactory.CreateDbContextAsync();
        var existingModels = (await db.Models
            .Select(m => new { m.Directory, m.FileName, m.Id, m.Checksum })
            .ToListAsync())
            .ToDictionary(m => (m.Directory, m.FileName), m => (m.Id, m.Checksum));

        var newCount = 0;
        var updatedCount = 0;
        foreach (var file in files)
        {
            var relativePath = Path.GetRelativePath(modelsPath, file);
            var fileName = Path.GetFileName(file);
            var directory = (Path.GetDirectoryName(relativePath) ?? "").Replace('\\', '/');
            var info = new FileInfo(file);
            var fileType = Path.GetExtension(file).TrimStart('.').ToLower();

            if (existingModels.TryGetValue((directory, fileName), out var existingEntry))
            {
                var currentChecksum = await ComputeChecksumAsync(file);
                if (currentChecksum == existingEntry.Checksum) continue;

                logger.LogInformation("Model content changed, updating: {FilePath}", file);

                var entity = await db.Models.FindAsync(existingEntry.Id);
                if (entity == null) continue;

                var data = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, currentChecksum);
                ApplyFileData(entity, data, info);
                await db.SaveChangesAsync();
                updatedCount++;
                continue;
            }

            // New file
            if (limit.HasValue && newCount >= limit.Value) continue;

            logger.LogInformation("Discovered {FileType} model: {FilePath}", fileType.ToUpper(), file);

            var newData = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs);
            var newEntity = new CachedModel
            {
                Id = Guid.NewGuid(),
                FileName = fileName,
                Directory = directory,
                FileType = fileType,
                DirectoryConfigId = newData.DirConfig?.Id,
            };
            ApplyFileData(newEntity, newData, info);
            db.Models.Add(newEntity);
            await db.SaveChangesAsync();
            existingModels[(directory, fileName)] = (newEntity.Id, newData.Checksum);
            newCount++;
        }

        logger.LogInformation("Scan complete: {New} new, {Updated} updated, {Total} total in cache.", newCount, updatedCount, await db.Models.CountAsync());
        return newCount;
    }

    private sealed record ModelFileData(
        string Checksum,
        LoadedGeometry? Geometry,
        string? PreviewImagePath,
        string? ConvexHull,
        string? ConcaveHull,
        string? ConvexSansRaftHull,
        ModelMetadataHelper.ComputedMetadata Metadata,
        DirectoryConfig? DirConfig);

    private async Task<ModelFileData> ComputeFileDataAsync(
        string file, string fileType, string directory,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        string? knownChecksum = null)
    {
        var checksum = knownChecksum ?? await ComputeChecksumAsync(file);
        var geometry = await loaderService.LoadModelAsync(file, fileType);
        var previewImagePath = geometry is not null
            ? await previewService.GeneratePreviewAsync(geometry, checksum)
            : null;
        var (convexHull, concaveHull, convexSansRaftHull) = geometry is not null
            ? await hullCalculationService.CalculateHullsAsync(geometry)
            : (null, null, null);
        directoryConfigs.TryGetValue(directory, out var dirConfig);
        var metadata = ModelMetadataHelper.Compute(file, dirConfig);
        return new ModelFileData(checksum, geometry, previewImagePath, convexHull, concaveHull, convexSansRaftHull, metadata, dirConfig);
    }

    private static void ApplyFileData(CachedModel entity, ModelFileData d, FileInfo info)
    {
        entity.Checksum                      = d.Checksum;
        entity.FileSize                      = info.Length;
        entity.FileModifiedAt                = info.LastWriteTimeUtc;
        entity.CachedAt                      = DateTime.UtcNow;
        entity.PreviewImagePath              = d.PreviewImagePath;
        entity.PreviewGeneratedAt            = d.PreviewImagePath != null ? DateTime.UtcNow : null;
        entity.ConvexHullCoordinates         = d.ConvexHull;
        entity.ConcaveHullCoordinates        = d.ConcaveHull;
        entity.ConvexSansRaftHullCoordinates = d.ConvexSansRaftHull;
        entity.HullGeneratedAt               = d.ConvexHull != null || d.ConcaveHull != null || d.ConvexSansRaftHull != null ? DateTime.UtcNow : null;
        entity.CalculatedCreator             = d.Metadata.Creator;
        entity.CalculatedCollection          = d.Metadata.Collection;
        entity.CalculatedSubcollection       = d.Metadata.Subcollection;
        entity.CalculatedCategory            = d.Metadata.Category;
        entity.CalculatedType                = d.Metadata.Type;
        entity.CalculatedSupported           = d.Metadata.Supported;
        entity.CalculatedModelName           = d.Metadata.ModelName;
        entity.DimensionXMm                  = d.Geometry?.DimensionXMm;
        entity.DimensionYMm                  = d.Geometry?.DimensionYMm;
        entity.DimensionZMm                  = d.Geometry?.DimensionZMm;
        entity.SphereCentreX                 = d.Geometry?.SphereCentre.X;
        entity.SphereCentreY                 = d.Geometry?.SphereCentre.Y;
        entity.SphereCentreZ                 = d.Geometry?.SphereCentre.Z;
        entity.SphereRadius                  = d.Geometry?.SphereRadius;
        entity.GeometryCalculatedAt          = d.Geometry is not null ? DateTime.UtcNow : null;
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
