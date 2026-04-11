using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class ModelService(
    IConfiguration config,
    ILoggerFactory loggerFactory,
    IDbContextFactory<ModelCacheContext> dbFactory,
    ModelLoaderService loaderService,
    ModelPreviewService previewService,
    HullCalculationService hullCalculationService,
    MetadataConfigService metadataConfigService,
    AppConfigService appConfigService)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Models);
    private static readonly string[] ModelExtensions = [".stl", ".obj"];

    public async Task<List<ModelDto>> GetModelsAsync(int? limit = null)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var modelsQuery = db.Models
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName);

        var models = await (limit.HasValue ? modelsQuery.Take(limit.Value) : modelsQuery).ToListAsync();

        return models.Select(m => m.ToModelDto()).ToList();
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
            .FirstOrDefaultAsync();

        return m?.ToModelDto();
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
        var defaultRaftHeightMm = await appConfigService.GetDefaultRaftHeightMmAsync();

        // Phase 3: Process new and updated model files
        await using var db = await dbFactory.CreateDbContextAsync();
        var existingModels = (await db.Models
            .Select(m => new { m.Directory, m.FileName, m.Id, m.Checksum, m.ScanConfigChecksum, m.PreviewGenerationVersion })
            .ToListAsync())
            .ToDictionary(m => (m.Directory, m.FileName), m => (m.Id, m.Checksum, m.ScanConfigChecksum, m.PreviewGenerationVersion));

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
                var expectedRaftHeightMm = ResolveRaftHeightMm(directory, directoryConfigs, defaultRaftHeightMm);
                var hullMetadataMismatch = NeedsHullRegeneration(existingEntry.ScanConfigChecksum, expectedRaftHeightMm);
                var previewStale = NeedsPreviewRegeneration(existingEntry.PreviewGenerationVersion);
                if (currentChecksum == existingEntry.Checksum && !hullMetadataMismatch && !previewStale) continue;

                var entity = await db.Models.FindAsync(existingEntry.Id);
                if (entity == null) continue;

                if (currentChecksum != existingEntry.Checksum)
                {
                    logger.LogInformation("Model content changed, updating: {FilePath}", file);
                    var data = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, currentChecksum, defaultRaftHeightMm);
                    ApplyFileData(entity, data, info);
                }
                else if (hullMetadataMismatch)
                {
                    logger.LogInformation(
                        "Hull config mismatch detected, regenerating hulls for: {FilePath} (stored checksum: {StoredChecksum}, expected checksum: {ExpectedChecksum})",
                        file,
                        entity.ScanConfigChecksum,
                        ScanConfig.Compute(expectedRaftHeightMm));
                    await RefreshHullDataAsync(entity, file, fileType, expectedRaftHeightMm);
                    if (previewStale)
                        await RefreshPreviewAsync(entity, file, fileType);
                    entity.CachedAt = DateTime.UtcNow;
                    entity.FileModifiedAt = info.LastWriteTimeUtc;
                    entity.FileSize = info.Length;
                }
                else
                {
                    logger.LogInformation(
                        "Preview version mismatch, regenerating preview for: {FilePath} (stored version: {StoredVersion}, current version: {CurrentVersion})",
                        file,
                        entity.PreviewGenerationVersion,
                        ModelPreviewService.CurrentPreviewGenerationVersion);
                    await RefreshPreviewAsync(entity, file, fileType);
                    entity.CachedAt = DateTime.UtcNow;
                    entity.FileModifiedAt = info.LastWriteTimeUtc;
                    entity.FileSize = info.Length;
                }

                await db.SaveChangesAsync();
                updatedCount++;
                continue;
            }

            // New file
            if (limit.HasValue && newCount >= limit.Value) continue;

            logger.LogInformation("Discovered {FileType} model: {FilePath}", fileType.ToUpper(), file);

            var newData = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, null, defaultRaftHeightMm);
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
            existingModels[(directory, fileName)] = (
                newEntity.Id,
                newData.Checksum,
                newEntity.ScanConfigChecksum,
                newEntity.PreviewGenerationVersion);
            newCount++;
        }

        logger.LogInformation("Scan complete: {New} new, {Updated} updated, {Total} total in cache.", newCount, updatedCount, await db.Models.CountAsync());
        return newCount;
    }

    public async Task<bool> ScanAndCacheSingleAsync(string relativeModelPath)
    {
        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath))
        {
            logger.LogWarning("Models:DirectoryPath is not configured");
            return false;
        }

        if (!Directory.Exists(modelsPath))
        {
            logger.LogWarning("Models directory not accessible: {Path}", modelsPath);
            return false;
        }

        if (string.IsNullOrWhiteSpace(relativeModelPath))
        {
            logger.LogWarning("Single-model indexing called with empty relative path");
            return false;
        }

        var normalized = relativeModelPath.Replace('\\', '/').TrimStart('/');
        var fullPath = Path.GetFullPath(Path.Combine(modelsPath, normalized.Replace('/', Path.DirectorySeparatorChar)));
        var rootPath = Path.GetFullPath(modelsPath);

        if (!fullPath.StartsWith(rootPath, StringComparison.OrdinalIgnoreCase))
        {
            logger.LogWarning("Rejected model path outside models root: {RelativePath}", relativeModelPath);
            return false;
        }

        if (!File.Exists(fullPath))
        {
            logger.LogWarning("Model file not found: {RelativePath}", relativeModelPath);
            return false;
        }

        var extension = Path.GetExtension(fullPath).ToLowerInvariant();
        if (!ModelExtensions.Contains(extension))
        {
            logger.LogWarning("Unsupported model extension for single-model indexing: {Path}", fullPath);
            return false;
        }

        var directory = (Path.GetDirectoryName(normalized) ?? "").Replace('\\', '/');
        var fileName = Path.GetFileName(fullPath);
        var fileType = extension.TrimStart('.');
        var info = new FileInfo(fullPath);

        var directoryConfigs = await metadataConfigService.EnsureDirectoryConfigsAsync(modelsPath, [directory]);
        var defaultRaftHeightMm = await appConfigService.GetDefaultRaftHeightMmAsync();
        var expectedRaftHeightMm = ResolveRaftHeightMm(directory, directoryConfigs, defaultRaftHeightMm);

        await using var db = await dbFactory.CreateDbContextAsync();
        var existing = await db.Models
            .FirstOrDefaultAsync(m => m.Directory == directory && m.FileName == fileName);

        var checksum = await ComputeChecksumAsync(fullPath);
        if (existing is not null
            && existing.Checksum == checksum
            && !NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm)
            && !NeedsPreviewRegeneration(existing.PreviewGenerationVersion))
            return false;

        if (existing is not null && existing.Checksum != checksum)
            logger.LogInformation("Model content changed, updating single file: {FilePath}", fullPath);
        else if (existing is not null && NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm))
            logger.LogInformation(
                "Single-model index refreshing hulls due to config mismatch: {FilePath} (stored checksum: {StoredChecksum}, expected checksum: {ExpectedChecksum})",
                fullPath,
                existing.ScanConfigChecksum,
                ScanConfig.Compute(expectedRaftHeightMm));
        else if (existing is not null)
            logger.LogInformation(
                "Single-model index refreshing preview due to version mismatch: {FilePath} (stored version: {StoredVersion}, current version: {CurrentVersion})",
                fullPath,
                existing.PreviewGenerationVersion,
                ModelPreviewService.CurrentPreviewGenerationVersion);
        else
            logger.LogInformation("Indexing single model file: {FilePath}", fullPath);

        if (existing is null)
        {
            var data = await ComputeFileDataAsync(fullPath, fileType, directory, directoryConfigs, checksum, defaultRaftHeightMm);
            existing = new CachedModel
            {
                Id = Guid.NewGuid(),
                FileName = fileName,
                Directory = directory,
                FileType = fileType,
                DirectoryConfigId = data.DirConfig?.Id,
            };
            ApplyFileData(existing, data, info);
            db.Models.Add(existing);
        }
        else
        {
            if (existing.Checksum != checksum)
            {
                var data = await ComputeFileDataAsync(fullPath, fileType, directory, directoryConfigs, checksum, defaultRaftHeightMm);
                existing.FileType = fileType;
                existing.DirectoryConfigId = data.DirConfig?.Id;
                ApplyFileData(existing, data, info);
            }
            else if (NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm))
            {
                existing.FileType = fileType;
                if (directoryConfigs.TryGetValue(directory, out var dirConfig))
                    existing.DirectoryConfigId = dirConfig.Id;
                await RefreshHullDataAsync(existing, fullPath, fileType, expectedRaftHeightMm);
                if (NeedsPreviewRegeneration(existing.PreviewGenerationVersion))
                    await RefreshPreviewAsync(existing, fullPath, fileType);
                existing.CachedAt = DateTime.UtcNow;
                existing.FileModifiedAt = info.LastWriteTimeUtc;
                existing.FileSize = info.Length;
            }
            else
            {
                await RefreshPreviewAsync(existing, fullPath, fileType);
                existing.CachedAt = DateTime.UtcNow;
                existing.FileModifiedAt = info.LastWriteTimeUtc;
                existing.FileSize = info.Length;
            }
        }

        await db.SaveChangesAsync();
        return true;
    }

    public async Task<List<RelatedModelDto>> GetOtherPartsAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var source = await db.Models.FirstOrDefaultAsync(m => m.Id == id);
        if (source == null) return [];

        if (string.IsNullOrWhiteSpace(source.CalculatedCreator))
            return [];

        var sourceCollection = NormalizePartGrouping(source.CalculatedCollection);
        var sourceSubcollection = NormalizePartGrouping(source.CalculatedSubcollection);

        var sourceName = string.IsNullOrWhiteSpace(source.CalculatedModelName)
            ? Path.GetFileNameWithoutExtension(source.FileName)
            : source.CalculatedModelName!;

        var candidates = await db.Models
            .Where(m => m.Id != id
                        && m.CalculatedCreator == source.CalculatedCreator)
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName)
            .ToListAsync();

        return candidates
            .Where(m => NormalizePartGrouping(m.CalculatedCollection) == sourceCollection
                        && NormalizePartGrouping(m.CalculatedSubcollection) == sourceSubcollection
                        && m.CalculatedSupported == source.CalculatedSupported
                        && string.Equals(
                            string.IsNullOrWhiteSpace(m.CalculatedModelName)
                                ? Path.GetFileNameWithoutExtension(m.FileName)
                                : m.CalculatedModelName,
                            sourceName,
                            StringComparison.OrdinalIgnoreCase))
            .Select(m => new RelatedModelDto(
                m.Id,
                m.CalculatedModelName ?? Path.GetFileNameWithoutExtension(m.FileName),
                string.IsNullOrEmpty(m.Directory) ? m.FileName : $"{m.Directory}/{m.FileName}",
                m.FileType,
                m.FileSize,
                m.PreviewImagePath != null ? $"/api/models/{m.Id}/preview?v={ModelPreviewService.CurrentPreviewGenerationVersion}" : null))
            .ToList();
    }

    private static string NormalizePartGrouping(string? value)
        => string.IsNullOrWhiteSpace(value) ? string.Empty : value.Trim();

    private sealed record ModelFileData(
        string Checksum,
        LoadedGeometry? Geometry,
        string? PreviewImagePath,
        string? ConvexHull,
        string? ConcaveHull,
        string? ConvexSansRaftHull,
        float RaftHeightMm,
        ModelMetadataHelper.ComputedMetadata Metadata,
        DirectoryConfig? DirConfig);

    private async Task<ModelFileData> ComputeFileDataAsync(
        string file, string fileType, string directory,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        string? knownChecksum = null,
        float defaultRaftHeightMm = HullCalculationService.DefaultRaftHeightMm)
    {
        var checksum = knownChecksum ?? await ComputeChecksumAsync(file);
        var geometry = await loaderService.LoadModelAsync(file, fileType);
        var previewImagePath = geometry is not null
            ? await previewService.GeneratePreviewAsync(geometry, checksum)
            : null;
        var raftHeightMm = ResolveRaftHeightMm(directory, directoryConfigs, defaultRaftHeightMm);
        var (convexHull, concaveHull, convexSansRaftHull) = geometry is not null
            ? await hullCalculationService.CalculateHullsAsync(geometry, raftHeightMm: raftHeightMm)
            : (null, null, null);
        directoryConfigs.TryGetValue(directory, out var dirConfig);
        var metadata = ModelMetadataHelper.Compute(file, dirConfig);
        return new ModelFileData(checksum, geometry, previewImagePath, convexHull, concaveHull, convexSansRaftHull, raftHeightMm, metadata, dirConfig);
    }

    private static void ApplyFileData(CachedModel entity, ModelFileData d, FileInfo info)
    {
        entity.Checksum = d.Checksum;
        entity.FileSize = info.Length;
        entity.FileModifiedAt = info.LastWriteTimeUtc;
        entity.CachedAt = DateTime.UtcNow;
        entity.PreviewImagePath = d.PreviewImagePath;
        entity.PreviewGeneratedAt = d.PreviewImagePath != null ? DateTime.UtcNow : null;
        entity.PreviewGenerationVersion = d.PreviewImagePath != null ? ModelPreviewService.CurrentPreviewGenerationVersion : null;
        ApplyHullData(entity, d.ConvexHull, d.ConcaveHull, d.ConvexSansRaftHull, d.Geometry is not null, d.RaftHeightMm);
        entity.ApplyCalculatedMetadata(d.Metadata);
        entity.DimensionXMm = ToFiniteOrNull(d.Geometry?.DimensionXMm);
        entity.DimensionYMm = ToFiniteOrNull(d.Geometry?.DimensionYMm);
        entity.DimensionZMm = ToFiniteOrNull(d.Geometry?.DimensionZMm);
        entity.SphereCentreX = ToFiniteOrNull(d.Geometry?.SphereCentre.X);
        entity.SphereCentreY = ToFiniteOrNull(d.Geometry?.SphereCentre.Y);
        entity.SphereCentreZ = ToFiniteOrNull(d.Geometry?.SphereCentre.Z);
        entity.SphereRadius = ToFiniteOrNull(d.Geometry?.SphereRadius);
        entity.GeometryCalculatedAt = d.Geometry is not null ? DateTime.UtcNow : null;
    }

    private static float? ToFiniteOrNull(float? value) =>
        value.HasValue && float.IsFinite(value.Value) ? value.Value : null;

    private async Task RefreshPreviewAsync(CachedModel entity, string filePath, string fileType)
    {
        var geometry = await loaderService.LoadModelAsync(filePath, fileType);
        if (geometry is null) return;

        var previewImagePath = await previewService.GeneratePreviewAsync(geometry, entity.Checksum);
        entity.PreviewImagePath = previewImagePath;
        entity.PreviewGeneratedAt = previewImagePath != null ? DateTime.UtcNow : null;
        entity.PreviewGenerationVersion = previewImagePath != null ? ModelPreviewService.CurrentPreviewGenerationVersion : null;
    }

    private async Task RefreshHullDataAsync(CachedModel entity, string filePath, string fileType, float raftHeightMm)
    {
        var geometry = await loaderService.LoadModelAsync(filePath, fileType);
        if (geometry is null)
        {
            ApplyHullData(entity, null, null, null, false);
            return;
        }

        var (convexHull, concaveHull, convexSansRaftHull) = await hullCalculationService.CalculateHullsAsync(geometry, raftHeightMm: raftHeightMm);
        ApplyHullData(entity, convexHull, concaveHull, convexSansRaftHull, true, raftHeightMm);
    }

    private static void ApplyHullData(
        CachedModel entity,
        string? convexHull,
        string? concaveHull,
        string? convexSansRaftHull,
        bool hullsCalculated,
        float raftHeightMm = HullCalculationService.DefaultRaftHeightMm)
    {
        entity.ConvexHullCoordinates = convexHull;
        entity.ConcaveHullCoordinates = concaveHull;
        entity.ConvexSansRaftHullCoordinates = convexSansRaftHull;
        entity.HullGeneratedAt = hullsCalculated ? DateTime.UtcNow : null;
        entity.HullGenerationVersion = hullsCalculated ? HullCalculationService.CurrentHullGenerationVersion : null;
        entity.HullRaftHeightMm = hullsCalculated ? raftHeightMm : null;
        entity.ScanConfigChecksum = hullsCalculated ? ScanConfig.Compute(raftHeightMm) : null;
    }

    private static bool NeedsHullRegeneration(string? storedChecksum, float expectedRaftHeightMm) =>
        storedChecksum != ScanConfig.Compute(expectedRaftHeightMm);

    private static bool NeedsPreviewRegeneration(int? storedVersion) =>
        storedVersion != ModelPreviewService.CurrentPreviewGenerationVersion;

    private static float ResolveRaftHeightMm(
        string directory,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        float defaultRaftHeightMm)
    {
        if (directoryConfigs.TryGetValue(directory, out var dirConfig)
            && dirConfig.RaftHeightMm.HasValue
            && float.IsFinite(dirConfig.RaftHeightMm.Value)
            && dirConfig.RaftHeightMm.Value >= 0f)
            return dirConfig.RaftHeightMm.Value;

        return defaultRaftHeightMm;
    }

    private static async Task<string> ComputeChecksumAsync(string filePath)
    {
        using var stream = File.OpenRead(filePath);
        var hash = await System.Security.Cryptography.SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLower();
    }

}
