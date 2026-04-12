using System.Collections.Concurrent;
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
    private static readonly string[] ModelExtensions = [".stl", ".obj", ".lys", ".lyt", ".ctb"];
    private static readonly HashSet<string> NonGeometryTypes =
        new(StringComparer.OrdinalIgnoreCase) { "lys", "lyt", "ctb" };
    private const int DefaultFileScanThreads = 10;

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

    public async Task<ModelDto?> UpdateModelMetadataAsync(Guid id, UpdateModelMetadataRequest request)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var model = await db.Models.FirstOrDefaultAsync(m => m.Id == id);
        if (model == null) return null;

        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath))
            throw new InvalidOperationException("Models:DirectoryPath is not configured.");

        var fullDirectoryPath = string.IsNullOrEmpty(model.Directory)
            ? modelsPath
            : Path.Combine(modelsPath, model.Directory.Replace('/', Path.DirectorySeparatorChar));
        var configPath = Path.Combine(fullDirectoryPath, DirectoryConfigReader.ConfigFileName);

        var data = await ReadConfigDataAsync(configPath);
        var modelMetadata = GetOrCreateModelMetadataNode(data);

        var sanitized = new ModelMetadataEntry(
            request.Name,
            request.PartName,
            request.Creator,
            request.Collection,
            request.Subcollection,
            request.Tags == null ? null : TagListHelper.Normalize(request.Tags),
            MetadataFieldRegistry.ValidateEnumValue("category", request.Category),
            MetadataFieldRegistry.ValidateEnumValue("type", request.Type),
            MetadataFieldRegistry.ValidateEnumValue("material", request.Material),
            request.Supported,
            request.RaftHeightMm);

        if (MetadataFieldRegistry.IsEmptyModelMetadataEntry(sanitized))
            modelMetadata.Remove(model.FileName);
        else
            modelMetadata[model.FileName] = MetadataFieldRegistry.ToYamlDictionary(sanitized);

        if (modelMetadata.Count == 0)
            RemoveKeyCaseInsensitive(data, "model_metadata");
        else
            data["model_metadata"] = modelMetadata;

        if (data.Count == 0)
        {
            if (File.Exists(configPath)) File.Delete(configPath);
        }
        else
        {
            var yaml = DirectoryConfigReader.YamlSerializer.Serialize(data);
            await File.WriteAllTextAsync(configPath, yaml);
        }

        var relativePath = string.IsNullOrEmpty(model.Directory)
            ? model.FileName
            : $"{model.Directory}/{model.FileName}";
        await ScanAndCacheSingleAsync(relativePath);

        return await GetModelDtoAsync(id);
    }

    public async Task<ModelMetadataDetail?> GetModelMetadataAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var model = await db.Models.FirstOrDefaultAsync(m => m.Id == id);
        if (model == null) return null;

        var dirConfig = await db.DirectoryConfigs
            .FirstOrDefaultAsync(d => d.DirectoryPath == model.Directory);

        // Per-model overrides stored in model_metadata section
        var configEntry = dirConfig != null
            ? ModelMetadataHelper.GetModelMetadataEntry(dirConfig, model.FileName)
            : null;

        var localValues = new ModelMetadataEntry(
            configEntry?.Name,
            configEntry?.PartName,
            configEntry?.Creator,
            configEntry?.Collection,
            configEntry?.Subcollection,
            configEntry?.Tags,
            configEntry?.Category,
            configEntry?.Type,
            configEntry?.Material,
            configEntry?.Supported,
            configEntry?.RaftHeightMm);

        // Compute inherited values from folder config (without per-model overrides)
        ModelMetadataEntry? inheritedValues = null;
        if (dirConfig != null)
        {
            var modelsPath = config["Models:DirectoryPath"];
            if (!string.IsNullOrEmpty(modelsPath))
            {
                var fullPath = string.IsNullOrEmpty(model.Directory)
                    ? Path.Combine(modelsPath, model.FileName)
                    : Path.Combine(modelsPath, model.Directory.Replace('/', Path.DirectorySeparatorChar), model.FileName);
                var inherited = ModelMetadataHelper.ComputeInherited(fullPath, dirConfig);
                inheritedValues = new ModelMetadataEntry(
                    inherited.ModelName,
                    inherited.PartName,
                    inherited.Creator,
                    inherited.Collection,
                    inherited.Subcollection,
                    inherited.Tags,
                    inherited.Category,
                    inherited.Type,
                    inherited.Material,
                    inherited.Supported,
                    inherited.RaftHeightMm);
            }
        }

        return new ModelMetadataDetail(localValues, inheritedValues);
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

        var fileScanThreads = ResolveFileScanThreads(config);
        logger.LogInformation(
            "Scanning {FileCount} model files with up to {ThreadCount} concurrent workers.",
            files.Count,
            fileScanThreads);

        var preparedResults = new ConcurrentBag<PreparedScanResult>();
        await Parallel.ForEachAsync(
            files,
            new ParallelOptions { MaxDegreeOfParallelism = fileScanThreads },
            async (file, _) =>
            {
                var prepared = await PrepareScanResultAsync(
                    file,
                    modelsPath,
                    existingModels,
                    directoryConfigs,
                    defaultRaftHeightMm);
                if (prepared is not null)
                    preparedResults.Add(prepared);
            });

        var orderedResults = preparedResults
            .OrderBy(r => r.RelativePath, StringComparer.OrdinalIgnoreCase)
            .ToList();

        var newCount = 0;
        var updatedCount = 0;
        foreach (var result in orderedResults)
        {
            if (result.ExistingId.HasValue)
            {
                var entity = await db.Models.FindAsync(result.ExistingId.Value);
                if (entity == null) continue;

                if (result.ContentUpdate is not null)
                {
                    logger.LogInformation("Model content changed, updating: {FilePath}", result.FilePath);
                    ApplyFileData(entity, result.ContentUpdate, result.FileInfo);
                }
                else if (result.GeometryRefresh is not null)
                {
                    ApplyGeometryRefreshData(entity, result.GeometryRefresh);
                    entity.CachedAt = DateTime.UtcNow;
                    entity.FileModifiedAt = result.FileInfo.LastWriteTimeUtc;
                    entity.FileSize = result.FileInfo.Length;
                }

                await db.SaveChangesAsync();
                updatedCount++;
                continue;
            }

            // New file
            if (limit.HasValue && newCount >= limit.Value) continue;

            logger.LogInformation("Discovered {FileType} model: {FilePath}", result.FileType.ToUpperInvariant(), result.FilePath);

            var newData = result.ContentUpdate;
            if (newData is null) continue;

            var newEntity = new CachedModel
            {
                Id = Guid.NewGuid(),
                FileName = result.FileName,
                Directory = result.Directory,
                FileType = result.FileType,
                DirectoryConfigId = newData.DirConfig?.Id,
            };
            ApplyFileData(newEntity, newData, result.FileInfo);
            db.Models.Add(newEntity);
            await db.SaveChangesAsync();
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
        var expectedRaftHeightMm = ResolveRaftHeightMmForModel(
            fullPath,
            directory,
            directoryConfigs,
            defaultRaftHeightMm);

        await using var db = await dbFactory.CreateDbContextAsync();
        var existing = await db.Models
            .FirstOrDefaultAsync(m => m.Directory == directory && m.FileName == fileName);

        var checksum = await ComputeChecksumAsync(fullPath);
        var previewStale = existing is not null
            && !IsNonGeometryType(fileType)
            && NeedsPreviewRegeneration(existing.PreviewGenerationVersion);
        if (existing is not null
            && existing.Checksum == checksum
            && (IsNonGeometryType(fileType) || !NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm))
            && (IsNonGeometryType(fileType) || !previewStale))
            return false;

        if (existing is not null && existing.Checksum != checksum)
            logger.LogInformation("Model content changed, updating single file: {FilePath}", fullPath);
        else if (existing is not null && !IsNonGeometryType(fileType) && NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm))
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
            else if (!IsNonGeometryType(fileType) && NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm))
            {
                existing.FileType = fileType;
                if (directoryConfigs.TryGetValue(directory, out var dirConfig))
                    existing.DirectoryConfigId = dirConfig.Id;
                var refresh = await ComputeGeometryRefreshDataAsync(
                    fullPath,
                    fileType,
                    existing.Checksum,
                    expectedRaftHeightMm,
                    includePreview: previewStale,
                    includeHulls: true);
                ApplyGeometryRefreshData(existing, refresh);
                existing.CachedAt = DateTime.UtcNow;
                existing.FileModifiedAt = info.LastWriteTimeUtc;
                existing.FileSize = info.Length;
            }
            else
            {
                var refresh = await ComputeGeometryRefreshDataAsync(
                    fullPath,
                    fileType,
                    existing.Checksum,
                    expectedRaftHeightMm,
                    includePreview: true,
                    includeHulls: false);
                ApplyGeometryRefreshData(existing, refresh);
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
                m.PreviewImagePath != null ? $"/api/models/{m.Id}/preview?v={m.PreviewGenerationVersion ?? 0}" : null))
            .ToList();
    }

    private static string NormalizePartGrouping(string? value)
        => string.IsNullOrWhiteSpace(value) ? string.Empty : value.Trim();

    private sealed record ModelFileData(
        string Checksum,
        string? PreviewImagePath,
        string? ConvexHull,
        string? ConcaveHull,
        string? ConvexSansRaftHull,
        float RaftHeightMm,
        ModelMetadataHelper.ComputedMetadata Metadata,
        DirectoryConfig? DirConfig,
        bool HasGeometry,
        float? DimensionXMm,
        float? DimensionYMm,
        float? DimensionZMm,
        float? SphereCentreX,
        float? SphereCentreY,
        float? SphereCentreZ,
        float? SphereRadius);

    private sealed record GeometryRefreshData(
        bool IncludePreview,
        bool IncludeHulls,
        bool GeometryLoaded,
        string? PreviewImagePath,
        string? ConvexHull,
        string? ConcaveHull,
        string? ConvexSansRaftHull,
        float RaftHeightMm);

    private sealed record PreparedScanResult(
        Guid? ExistingId,
        string FilePath,
        string RelativePath,
        string Directory,
        string FileName,
        string FileType,
        FileInfo FileInfo,
        ModelFileData? ContentUpdate,
        GeometryRefreshData? GeometryRefresh);

    private async Task<PreparedScanResult?> PrepareScanResultAsync(
        string file,
        string modelsPath,
        IReadOnlyDictionary<(string Directory, string FileName), (Guid Id, string Checksum, string? ScanConfigChecksum, int? PreviewGenerationVersion)> existingModels,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        float defaultRaftHeightMm)
    {
        var relativePath = Path.GetRelativePath(modelsPath, file);
        var fileName = Path.GetFileName(file);
        var directory = (Path.GetDirectoryName(relativePath) ?? "").Replace('\\', '/');
        var info = new FileInfo(file);
        var fileType = Path.GetExtension(file).TrimStart('.').ToLowerInvariant();

        if (existingModels.TryGetValue((directory, fileName), out var existingEntry))
        {
            var currentChecksum = await ComputeChecksumAsync(file);
            var expectedRaftHeightMm = ResolveRaftHeightMmForModel(
                file,
                directory,
                directoryConfigs,
                defaultRaftHeightMm);
            var hullMetadataMismatch = !IsNonGeometryType(fileType)
                && NeedsHullRegeneration(existingEntry.ScanConfigChecksum, expectedRaftHeightMm);
            var previewStale = !IsNonGeometryType(fileType)
                && NeedsPreviewRegeneration(existingEntry.PreviewGenerationVersion);
            if (currentChecksum == existingEntry.Checksum && !hullMetadataMismatch && !previewStale)
                return null;

            if (currentChecksum != existingEntry.Checksum)
            {
                var data = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, currentChecksum, defaultRaftHeightMm);
                return new PreparedScanResult(existingEntry.Id, file, relativePath, directory, fileName, fileType, info, data, null);
            }

            if (hullMetadataMismatch)
            {
                logger.LogInformation(
                    "Hull config mismatch detected, regenerating hulls for: {FilePath} (stored checksum: {StoredChecksum}, expected checksum: {ExpectedChecksum})",
                    file,
                    existingEntry.ScanConfigChecksum,
                    ScanConfig.Compute(expectedRaftHeightMm));

                var refresh = await ComputeGeometryRefreshDataAsync(
                    file,
                    fileType,
                    currentChecksum,
                    expectedRaftHeightMm,
                    includePreview: previewStale,
                    includeHulls: true);
                return new PreparedScanResult(existingEntry.Id, file, relativePath, directory, fileName, fileType, info, null, refresh);
            }

            logger.LogInformation(
                "Preview version mismatch, regenerating preview for: {FilePath} (stored version: {StoredVersion}, current version: {CurrentVersion})",
                file,
                existingEntry.PreviewGenerationVersion,
                ModelPreviewService.CurrentPreviewGenerationVersion);

            var previewRefresh = await ComputeGeometryRefreshDataAsync(
                file,
                fileType,
                currentChecksum,
                expectedRaftHeightMm,
                includePreview: true,
                includeHulls: false);
            return new PreparedScanResult(existingEntry.Id, file, relativePath, directory, fileName, fileType, info, null, previewRefresh);
        }

        var newData = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, null, defaultRaftHeightMm);
        return new PreparedScanResult(null, file, relativePath, directory, fileName, fileType, info, newData, null);
    }

    private async Task<ModelFileData> ComputeFileDataAsync(
        string file, string fileType, string directory,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        string? knownChecksum = null,
        float defaultRaftHeightMm = HullCalculationService.DefaultRaftHeightMm)
    {
        var checksum = knownChecksum ?? await ComputeChecksumAsync(file);
        directoryConfigs.TryGetValue(directory, out var dirConfig);
        var metadata = ModelMetadataHelper.Compute(file, dirConfig);
        var raftHeightMm = metadata.RaftHeightMm.HasValue
                   && float.IsFinite(metadata.RaftHeightMm.Value)
                   && metadata.RaftHeightMm.Value >= 0f
            ? metadata.RaftHeightMm.Value
            : ResolveRaftHeightMm(directory, directoryConfigs, defaultRaftHeightMm);

        // Skip expensive geometry pipeline for file types that do not contain mesh geometry.
        LoadedGeometry? geometry = null;
        string? previewImagePath = null;
        string? convexHull = null, concaveHull = null, convexSansRaftHull = null;
        float? dimensionXMm = null, dimensionYMm = null, dimensionZMm = null;
        float? sphereCentreX = null, sphereCentreY = null, sphereCentreZ = null, sphereRadius = null;

        if (!IsNonGeometryType(fileType))
        {
            geometry = await loaderService.LoadModelAsync(file, fileType);
            previewImagePath = geometry is not null
                ? await previewService.GeneratePreviewAsync(geometry, checksum)
                : null;

            if (geometry is not null)
            {
                (convexHull, concaveHull, convexSansRaftHull) =
                    await hullCalculationService.CalculateHullsAsync(geometry, raftHeightMm: raftHeightMm);
                dimensionXMm = ToFiniteOrNull(geometry.DimensionXMm);
                dimensionYMm = ToFiniteOrNull(geometry.DimensionYMm);
                dimensionZMm = ToFiniteOrNull(geometry.DimensionZMm);
                sphereCentreX = ToFiniteOrNull(geometry.SphereCentre.X);
                sphereCentreY = ToFiniteOrNull(geometry.SphereCentre.Y);
                sphereCentreZ = ToFiniteOrNull(geometry.SphereCentre.Z);
                sphereRadius = ToFiniteOrNull(geometry.SphereRadius);
            }
        }

        return new ModelFileData(
            checksum,
            previewImagePath,
            convexHull,
            concaveHull,
            convexSansRaftHull,
            raftHeightMm,
            metadata,
            dirConfig,
            geometry is not null,
            dimensionXMm,
            dimensionYMm,
            dimensionZMm,
            sphereCentreX,
            sphereCentreY,
            sphereCentreZ,
            sphereRadius);
    }

    private static bool IsNonGeometryType(string fileType) => NonGeometryTypes.Contains(fileType);

    private static void ApplyFileData(CachedModel entity, ModelFileData d, FileInfo info)
    {
        entity.Checksum = d.Checksum;
        entity.FileSize = info.Length;
        entity.FileModifiedAt = info.LastWriteTimeUtc;
        entity.CachedAt = DateTime.UtcNow;
        entity.PreviewImagePath = d.PreviewImagePath;
        entity.PreviewGeneratedAt = d.PreviewImagePath != null ? DateTime.UtcNow : null;
        entity.PreviewGenerationVersion = d.PreviewImagePath != null ? ModelPreviewService.CurrentPreviewGenerationVersion : null;
        ApplyHullData(entity, d.ConvexHull, d.ConcaveHull, d.ConvexSansRaftHull, d.HasGeometry, d.RaftHeightMm);
        entity.ApplyCalculatedMetadata(d.Metadata);
        entity.DimensionXMm = d.DimensionXMm;
        entity.DimensionYMm = d.DimensionYMm;
        entity.DimensionZMm = d.DimensionZMm;
        entity.SphereCentreX = d.SphereCentreX;
        entity.SphereCentreY = d.SphereCentreY;
        entity.SphereCentreZ = d.SphereCentreZ;
        entity.SphereRadius = d.SphereRadius;
        entity.GeometryCalculatedAt = d.HasGeometry ? DateTime.UtcNow : null;
    }

    private static float? ToFiniteOrNull(float? value) =>
        value.HasValue && float.IsFinite(value.Value) ? value.Value : null;

    private static void ApplyGeometryRefreshData(CachedModel entity, GeometryRefreshData refresh)
    {
        if (refresh.IncludeHulls)
            ApplyHullData(entity, refresh.ConvexHull, refresh.ConcaveHull, refresh.ConvexSansRaftHull, refresh.GeometryLoaded, refresh.RaftHeightMm);

        if (!refresh.IncludePreview || !refresh.GeometryLoaded)
            return;

        entity.PreviewImagePath = refresh.PreviewImagePath;
        entity.PreviewGeneratedAt = refresh.PreviewImagePath != null ? DateTime.UtcNow : null;
        entity.PreviewGenerationVersion = refresh.PreviewImagePath != null ? ModelPreviewService.CurrentPreviewGenerationVersion : null;
    }

    private async Task<GeometryRefreshData> ComputeGeometryRefreshDataAsync(
        string filePath,
        string fileType,
        string checksum,
        float raftHeightMm,
        bool includePreview,
        bool includeHulls)
    {
        var geometry = await loaderService.LoadModelAsync(filePath, fileType);
        if (geometry is null)
            return new GeometryRefreshData(includePreview, includeHulls, false, null, null, null, null, raftHeightMm);

        string? previewImagePath = null;
        if (includePreview)
            previewImagePath = await previewService.GeneratePreviewAsync(geometry, checksum);

        string? convexHull = null;
        string? concaveHull = null;
        string? convexSansRaftHull = null;
        if (includeHulls)
            (convexHull, concaveHull, convexSansRaftHull) =
                await hullCalculationService.CalculateHullsAsync(geometry, raftHeightMm: raftHeightMm);

        return new GeometryRefreshData(
            includePreview,
            includeHulls,
            true,
            previewImagePath,
            convexHull,
            concaveHull,
            convexSansRaftHull,
            raftHeightMm);
    }

    private static int ResolveFileScanThreads(IConfiguration configuration)
    {
        var configuredValue = configuration.GetValue<int?>("Indexing:FileScanThreads");
        return configuredValue is > 0
            ? configuredValue.Value
            : DefaultFileScanThreads;
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

    private static float ResolveRaftHeightMmForModel(
        string fullFilePath,
        string directory,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        float defaultRaftHeightMm)
    {
        directoryConfigs.TryGetValue(directory, out var dirConfig);
        var computed = ModelMetadataHelper.Compute(fullFilePath, dirConfig);
        if (computed.RaftHeightMm.HasValue
            && float.IsFinite(computed.RaftHeightMm.Value)
            && computed.RaftHeightMm.Value >= 0f)
            return computed.RaftHeightMm.Value;

        return ResolveRaftHeightMm(directory, directoryConfigs, defaultRaftHeightMm);
    }

    private static async Task<string> ComputeChecksumAsync(string filePath)
    {
        using var stream = File.OpenRead(filePath);
        var hash = await System.Security.Cryptography.SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLower();
    }

    private static async Task<Dictionary<string, object>> ReadConfigDataAsync(string configPath)
    {
        if (!File.Exists(configPath)) return [];
        var yaml = await File.ReadAllTextAsync(configPath);
        return DirectoryConfigReader.YamlDeserializer.Deserialize<Dictionary<string, object>>(yaml) ?? [];
    }

    private static Dictionary<object, object> GetOrCreateModelMetadataNode(Dictionary<string, object> data)
    {
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, "model_metadata", StringComparison.OrdinalIgnoreCase)) continue;
            if (kvp.Value is Dictionary<object, object> dict) return dict;
            if (kvp.Value is Dictionary<string, object> dictString)
                return dictString.ToDictionary(k => (object)k.Key, v => v.Value);
            break;
        }

        return new Dictionary<object, object>();
    }


    private static void RemoveKeyCaseInsensitive(Dictionary<string, object> data, string key)
    {
        var matched = data.Keys.FirstOrDefault(k => string.Equals(k, key, StringComparison.OrdinalIgnoreCase));
        if (matched != null)
            data.Remove(matched);
    }

}
