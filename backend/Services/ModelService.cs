using System.Threading.Channels;
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
    AppConfigService appConfigService,
    TagGenerationService tagGenerationService)
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

    public async Task<List<ModelDto>> GetModelsByIdsAsync(IReadOnlyCollection<Guid> ids)
    {
        if (ids.Count == 0)
            return [];

        var idSet = ids.ToHashSet();

        await using var db = await dbFactory.CreateDbContextAsync();
        var models = await db.Models
            .AsNoTracking()
            .Where(m => idSet.Contains(m.Id))
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName)
            .ToListAsync();

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

    public async Task<int> ScanAndCacheAsync(
        int? limit = null,
        string? directoryFilter = null,
        IIndexingProgressReporter? progressReporter = null,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();

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

        if (progressReporter is not null)
        {
            await progressReporter.OnScanStartedAsync(files.Count);
            await progressReporter.OnFilesDiscoveredAsync(files
                .Select(f => new IndexingFilePlan(
                    Path.GetRelativePath(modelsPath, f).Replace('\\', '/'),
                    Path.GetExtension(f).TrimStart('.').ToLowerInvariant()))
                .ToList());
        }

        cancellationToken.ThrowIfCancellationRequested();

        var relativeDirectories = files
            .Select(f => (Path.GetDirectoryName(Path.GetRelativePath(modelsPath, f)) ?? "").Replace('\\', '/'))
            .Distinct(StringComparer.Ordinal)
            .ToList();

        // Phase 2: Sync DirectoryConfig records (root-first, hash-detected changes)
        var directoryConfigs = await metadataConfigService.EnsureDirectoryConfigsAsync(modelsPath, relativeDirectories);
        var appConfig = await appConfigService.GetAsync();
        var defaultRaftHeightMm = appConfig.DefaultRaftHeightMm;
        var generateTagsOnScan = appConfig.TagGenerationEnabled;
        var generateDescriptionsOnScan = appConfig.AiDescriptionEnabled;

        // Phase 3: Process new and updated model files
        await using var db = await dbFactory.CreateDbContextAsync();
        var configuredTagSchema = generateTagsOnScan
            ? TagListHelper.Normalize(await db.MetadataDictionaryValues
                .AsNoTracking()
                .Where(v => v.Field == "tags")
                .OrderBy(v => v.Value)
                .Select(v => v.Value)
                .ToListAsync())
            : [];
        logger.LogDebug(
            "Scan AI-generation settings: tagsEnabled={TagsEnabled}, descriptionEnabled={DescriptionEnabled}, provider={Provider}, schemaCount={SchemaCount}",
            generateTagsOnScan,
            generateDescriptionsOnScan,
            appConfig.TagGenerationProvider,
            configuredTagSchema.Count);

        var existingModels = (await db.Models
            .Select(m => new ExistingModelScanState(
                m.Id,
                m.Directory,
                m.FileName,
                m.Checksum,
                m.FileModifiedAt,
                m.ScanConfigChecksum,
                m.PreviewGenerationVersion,
                m.PreviewGeneratedAt,
                m.GeneratedTagsChecksum,
                m.GeneratedTagsStatus,
                m.GeneratedTagsJson,
                m.GeneratedTagsAt,
                m.GeneratedDescription,
                m.GeneratedDescriptionChecksum,
                m.GeneratedDescriptionAt,
                m.PreviewImagePath,
                m.CalculatedModelName,
                m.CalculatedPartName,
                m.CalculatedCategory,
                m.CalculatedType,
                m.CalculatedMaterial,
                m.CalculatedCreator,
                m.CalculatedCollection,
                m.CalculatedSubcollection))
            .ToListAsync())
            .ToDictionary(m => (m.Directory, m.FileName), m => m);

        var fileScanThreads = ResolveFileScanThreads(config);
        logger.LogInformation(
            "Scanning {FileCount} model files with up to {ThreadCount} concurrent workers.",
            files.Count,
            fileScanThreads);

        var preparedResults = Channel.CreateUnbounded<PreparedScanResult>(
            new UnboundedChannelOptions
            {
                SingleReader = true,
                SingleWriter = false,
            });

        var producer = Task.Run(async () =>
        {
            try
            {
                await Parallel.ForEachAsync(
                    files,
                    new ParallelOptions
                    {
                        MaxDegreeOfParallelism = fileScanThreads,
                        CancellationToken = cancellationToken,
                    },
                    async (file, ct) =>
                    {
                        var prepareTimer = System.Diagnostics.Stopwatch.StartNew();
                        var prepared = await PrepareScanResultAsync(
                            file,
                            modelsPath,
                            existingModels,
                            directoryConfigs,
                            defaultRaftHeightMm,
                            appConfig,
                            configuredTagSchema,
                            generateTagsOnScan,
                            generateDescriptionsOnScan);
                        if (prepared is not null)
                            preparedResults.Writer.TryWrite(prepared with
                            {
                                PreparationDurationMs = prepareTimer.Elapsed.TotalMilliseconds,
                            });
                    });

                preparedResults.Writer.TryComplete();
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                // Cancellation is expected when a run is explicitly cancelled.
                preparedResults.Writer.TryComplete();
            }
            catch (Exception ex)
            {
                preparedResults.Writer.TryComplete(ex);
            }
        });

        var processedRelativePaths = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        var newCount = 0;
        var updatedCount = 0;
        try
        {
            await foreach (var result in preparedResults.Reader.ReadAllAsync(cancellationToken))
            {
                cancellationToken.ThrowIfCancellationRequested();
                var perFileTimer = System.Diagnostics.Stopwatch.StartNew();
                processedRelativePaths.Add(result.RelativePath);

                if (result.ExistingId.HasValue)
                {
                    var entity = await db.Models.FindAsync(result.ExistingId.Value);
                    if (entity == null) continue;

                    var generatedPreview = false;
                    var generatedHull = false;
                    var generatedAiTags = false;
                    var generatedAiDescription = false;
                    var aiGenerationReason = result.AiGenerationReason;
                    var status = "processed";
                    var message = "Updated indexed model";
                    var wasUpdated = false;

                    if (result.ContentUpdate is not null)
                    {
                        logger.LogInformation("Model content changed, updating: {FilePath}", result.FilePath);
                        ApplyFileData(entity, result.ContentUpdate, result.FileInfo);
                        generatedPreview = result.ContentUpdate.PreviewGenerated;
                        generatedHull = result.ContentUpdate.HullGenerated;
                        wasUpdated = true;
                        message = "Content changed; rebuilt cached metadata and geometry";
                    }
                    else if (result.GeometryRefresh is not null)
                    {
                        ApplyGeometryRefreshData(entity, result.GeometryRefresh);
                        entity.CachedAt = DateTime.UtcNow;
                        entity.FileModifiedAt = result.FileInfo.LastWriteTimeUtc;
                        entity.FileSize = result.FileInfo.Length;
                        generatedPreview = result.GeometryRefresh.PreviewGenerated;
                        generatedHull = result.GeometryRefresh.HullGenerated;
                        wasUpdated = true;
                        message = result.GeometryRefresh.IncludeHulls
                            ? "Regenerated hulls/preview from config or version changes"
                            : "Regenerated preview from version changes";
                    }
                    else if (result.FileMetadataOnlyRefresh)
                    {
                        entity.CachedAt = DateTime.UtcNow;
                        entity.FileModifiedAt = result.FileInfo.LastWriteTimeUtc;
                        entity.FileSize = result.FileInfo.Length;
                        wasUpdated = true;
                        message = "Updated file timestamp after checksum verification";
                    }

                    if (wasUpdated)
                        await db.SaveChangesAsync();

                    var tagsNeedGeneration = false;
                    var descriptionNeedGeneration = false;
                    if (generateTagsOnScan || generateDescriptionsOnScan)
                    {
                        tagsNeedGeneration = generateTagsOnScan
                            && TagGenerationService.NeedsRegeneration(entity, appConfig, configuredTagSchema);
                        descriptionNeedGeneration = generateDescriptionsOnScan
                            && TagGenerationService.NeedsDescriptionRegeneration(entity, appConfig);
                    }

                    var shouldGenerateAi = tagsNeedGeneration || descriptionNeedGeneration;
                    if (shouldGenerateAi)
                    {
                        aiGenerationReason = result.ContentUpdate is not null || result.GeometryRefresh?.PreviewGenerated == true
                            ? (result.ExistingId.HasValue ? "preview updated" : "new model preview")
                            : tagsNeedGeneration && descriptionNeedGeneration
                                ? "tags+description stale"
                                : tagsNeedGeneration
                                    ? "tags checksum stale"
                                    : "description checksum stale";

                        var tagResult = await TryGenerateTagsOnScanAsync(entity.Id, generateTagsOnScan, generateDescriptionsOnScan);
                        generatedAiTags = tagResult.GeneratedTags;
                        generatedAiDescription = tagResult.GeneratedDescription;
                        if (!tagResult.Success)
                        {
                            status = "failed";
                            message = tagResult.Error ?? "AI generation failed";
                        }
                        else if (generatedAiTags || generatedAiDescription)
                        {
                            message = "Regenerated AI metadata";
                        }
                    }

                    if (result.ContentUpdate is null && result.GeometryRefresh is null && !shouldGenerateAi)
                    {
                        status = "skipped";
                        message = "No cache changes required";
                    }

                    if (progressReporter is not null)
                    {
                        await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                            result.RelativePath,
                            result.FileType,
                            status,
                            IsNew: false,
                            WasUpdated: wasUpdated,
                            GeneratedPreview: generatedPreview,
                            GeneratedHull: generatedHull,
                            GeneratedAiTags: generatedAiTags,
                            GeneratedAiDescription: generatedAiDescription,
                            AiGenerationReason: generatedAiTags || generatedAiDescription ? aiGenerationReason : null,
                            Message: message,
                            DurationMs: result.PreparationDurationMs + perFileTimer.Elapsed.TotalMilliseconds));
                    }
                    updatedCount++;
                    continue;
                }

                // New file
                if (limit.HasValue && newCount >= limit.Value)
                {
                    if (progressReporter is not null)
                    {
                        await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                            result.RelativePath,
                            result.FileType,
                            "skipped",
                            IsNew: true,
                            WasUpdated: false,
                            GeneratedPreview: false,
                            GeneratedHull: false,
                            GeneratedAiTags: false,
                            GeneratedAiDescription: false,
                            AiGenerationReason: null,
                            Message: "Skipped due to scan limit",
                            DurationMs: perFileTimer.Elapsed.TotalMilliseconds));
                    }
                    continue;
                }

                logger.LogInformation("Discovered {FileType} model: {FilePath}", result.FileType.ToUpperInvariant(), result.FilePath);

                var newData = result.ContentUpdate;
                if (newData is null)
                {
                    if (progressReporter is not null)
                    {
                        await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                            result.RelativePath,
                            result.FileType,
                            "failed",
                            IsNew: true,
                            WasUpdated: false,
                            GeneratedPreview: false,
                            GeneratedHull: false,
                            GeneratedAiTags: false,
                            GeneratedAiDescription: false,
                            AiGenerationReason: null,
                            Message: "Unable to compute file data",
                            DurationMs: perFileTimer.Elapsed.TotalMilliseconds));
                    }
                    continue;
                }

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
                var generatedAiTagsNew = false;
                var generatedAiDescriptionNew = false;
                string? aiGenerationReasonNew = null;
                var tagsNeedGenerationNew = false;
                var descriptionNeedGenerationNew = false;
                if (generateTagsOnScan || generateDescriptionsOnScan)
                {
                    tagsNeedGenerationNew = generateTagsOnScan
                        && TagGenerationService.NeedsRegeneration(newEntity, appConfig, configuredTagSchema);
                    descriptionNeedGenerationNew = generateDescriptionsOnScan
                        && TagGenerationService.NeedsDescriptionRegeneration(newEntity, appConfig);

                    if (tagsNeedGenerationNew || descriptionNeedGenerationNew)
                    {
                        aiGenerationReasonNew = "new model preview";
                        var tagResult = await TryGenerateTagsOnScanAsync(newEntity.Id, generateTagsOnScan, generateDescriptionsOnScan);
                        generatedAiTagsNew = tagResult.GeneratedTags;
                        generatedAiDescriptionNew = tagResult.GeneratedDescription;
                    }
                }

                if (progressReporter is not null)
                {
                    await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                        result.RelativePath,
                        result.FileType,
                        "processed",
                        IsNew: true,
                        WasUpdated: false,
                        GeneratedPreview: newData.PreviewImagePath is not null,
                        GeneratedHull: newData.ConvexHull is not null
                            || newData.ConcaveHull is not null
                            || newData.ConvexSansRaftHull is not null,
                        GeneratedAiTags: generatedAiTagsNew,
                        GeneratedAiDescription: generatedAiDescriptionNew,
                        AiGenerationReason: generatedAiTagsNew || generatedAiDescriptionNew ? aiGenerationReasonNew : null,
                        Message: "Indexed new model file",
                        DurationMs: result.PreparationDurationMs + perFileTimer.Elapsed.TotalMilliseconds));
                }
                newCount++;
            }
        }
        finally
        {
            try
            {
                await producer;
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                // Expected when the scan run is cancelled.
            }
        }

        var unchangedFiles = files
            .Select(f => Path.GetRelativePath(modelsPath, f).Replace('\\', '/'))
            .Where(relative => !processedRelativePaths.Contains(relative));
        if (progressReporter is not null)
        {
            foreach (var unchangedRelativePath in unchangedFiles)
            {
                cancellationToken.ThrowIfCancellationRequested();
                await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                    unchangedRelativePath,
                    Path.GetExtension(unchangedRelativePath).TrimStart('.').ToLowerInvariant(),
                    "skipped",
                    IsNew: false,
                    WasUpdated: false,
                    GeneratedPreview: false,
                    GeneratedHull: false,
                    GeneratedAiTags: false,
                    GeneratedAiDescription: false,
                    AiGenerationReason: null,
                    Message: "Already up-to-date",
                    DurationMs: 0));
            }
        }

        logger.LogInformation("Scan complete: {New} new, {Updated} updated, {Total} total in cache.", newCount, updatedCount, await db.Models.CountAsync());
        return newCount;
    }

    public async Task<bool> ScanAndCacheSingleAsync(
        string relativeModelPath,
        IIndexingProgressReporter? progressReporter = null,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();

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
        var singleFileTimer = System.Diagnostics.Stopwatch.StartNew();

        if (progressReporter is not null)
        {
            await progressReporter.OnScanStartedAsync(1);
            await progressReporter.OnFilesDiscoveredAsync([new IndexingFilePlan(normalized, fileType)]);
        }

        var directoryConfigs = await metadataConfigService.EnsureDirectoryConfigsAsync(modelsPath, [directory]);
        var appConfig = await appConfigService.GetAsync();
        var defaultRaftHeightMm = appConfig.DefaultRaftHeightMm;
        var generateTagsOnScan = appConfig.TagGenerationEnabled;
        var generateDescriptionsOnScan = appConfig.AiDescriptionEnabled;
        cancellationToken.ThrowIfCancellationRequested();
        await using var db = await dbFactory.CreateDbContextAsync();
        var configuredTagSchema = generateTagsOnScan
            ? TagListHelper.Normalize(await db.MetadataDictionaryValues
                .AsNoTracking()
                .Where(v => v.Field == "tags")
                .OrderBy(v => v.Value)
                .Select(v => v.Value)
                .ToListAsync())
            : [];
        var expectedRaftHeightMm = ResolveRaftHeightMmForModel(
            fullPath,
            directory,
            directoryConfigs,
            defaultRaftHeightMm);

        var existing = await db.Models
            .FirstOrDefaultAsync(m => m.Directory == directory && m.FileName == fileName);
        var isNewModel = existing is null;

        var checksumEvaluation = await ResolveChecksumAsync(
            fullPath,
            info.LastWriteTimeUtc,
            existing?.Checksum,
            existing?.FileModifiedAt);
        var checksum = checksumEvaluation.Checksum;
        cancellationToken.ThrowIfCancellationRequested();
        var checksumChanged = existing is null || existing.Checksum != checksum;
        var previewStale = existing is not null
            && !IsNonGeometryType(fileType)
            && NeedsPreviewRegeneration(existing.PreviewGenerationVersion);
        var hullStale = existing is not null
            && !IsNonGeometryType(fileType)
            && NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm);
        var tagsStale = existing is not null
            && generateTagsOnScan
            && TagGenerationService.NeedsRegeneration(existing, appConfig, configuredTagSchema);
        var descriptionStale = existing is not null
            && generateDescriptionsOnScan
            && TagGenerationService.NeedsDescriptionRegeneration(existing, appConfig);
        var expectedTagChecksum = existing is not null && generateTagsOnScan && configuredTagSchema.Count > 0
            ? TagGenerationService.ComputeGenerationChecksum(existing, appConfig, configuredTagSchema)
            : null;
        var expectedDescriptionChecksum = existing is not null && generateDescriptionsOnScan
            ? TagGenerationService.ComputeDescriptionChecksum(existing, appConfig)
            : null;
        var storedTagChecksum = existing?.GeneratedTagsChecksum;
        var storedDescriptionChecksum = existing?.GeneratedDescriptionChecksum;
        if (existing is not null
            && existing.Checksum == checksum
            && (IsNonGeometryType(fileType) || !hullStale)
            && (IsNonGeometryType(fileType) || !previewStale)
            && !tagsStale
            && !descriptionStale)
        {
            if (checksumEvaluation.Recomputed)
            {
                existing.FileModifiedAt = info.LastWriteTimeUtc;
                existing.FileSize = info.Length;
                existing.CachedAt = DateTime.UtcNow;
                await db.SaveChangesAsync(cancellationToken);
            }

            logger.LogDebug(
                "Single-model scan skipped changes for {FilePath}; checksum/hulls/preview/tags/description are up-to-date. tagStatus={TagStatus}, tagJsonPresent={TagJsonPresent}, storedTagChecksum={StoredTagChecksum}, expectedTagChecksum={ExpectedTagChecksum}, descriptionPresent={DescriptionPresent}, storedDescriptionChecksum={StoredDescriptionChecksum}, expectedDescriptionChecksum={ExpectedDescriptionChecksum}",
                fullPath,
                existing.GeneratedTagsStatus,
                !string.IsNullOrWhiteSpace(existing.GeneratedTagsJson),
                storedTagChecksum,
                expectedTagChecksum,
                !string.IsNullOrWhiteSpace(existing.GeneratedDescription),
                storedDescriptionChecksum,
                expectedDescriptionChecksum);
            if (progressReporter is not null)
            {
                await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                    normalized,
                    fileType,
                    "skipped",
                    IsNew: false,
                    WasUpdated: false,
                    GeneratedPreview: false,
                    GeneratedHull: false,
                    GeneratedAiTags: false,
                    GeneratedAiDescription: false,
                    AiGenerationReason: null,
                    Message: "Already up-to-date",
                    DurationMs: singleFileTimer.Elapsed.TotalMilliseconds));
            }
            return false;
        }

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

        var generatedPreview = false;
        var generatedHull = false;

        if (existing is null)
        {
            cancellationToken.ThrowIfCancellationRequested();
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
            generatedPreview = data.PreviewGenerated;
            generatedHull = data.HullGenerated;
            db.Models.Add(existing);
            if (progressReporter is not null)
            {
                await progressReporter.OnLogAsync("info", "Discovered new model", normalized);
            }
        }
        else
        {
            if (existing.Checksum != checksum)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var data = await ComputeFileDataAsync(fullPath, fileType, directory, directoryConfigs, checksum, defaultRaftHeightMm);
                existing.FileType = fileType;
                existing.DirectoryConfigId = data.DirConfig?.Id;
                ApplyFileData(existing, data, info);
                generatedPreview = data.PreviewGenerated;
                generatedHull = data.HullGenerated;
            }
            else if (!IsNonGeometryType(fileType) && NeedsHullRegeneration(existing.ScanConfigChecksum, expectedRaftHeightMm))
            {
                cancellationToken.ThrowIfCancellationRequested();
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
                generatedPreview = refresh.PreviewGenerated;
                generatedHull = refresh.HullGenerated;
                existing.CachedAt = DateTime.UtcNow;
                existing.FileModifiedAt = info.LastWriteTimeUtc;
                existing.FileSize = info.Length;
            }
            else if (!IsNonGeometryType(fileType) && previewStale)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var refresh = await ComputeGeometryRefreshDataAsync(
                    fullPath,
                    fileType,
                    existing.Checksum,
                    expectedRaftHeightMm,
                    includePreview: true,
                    includeHulls: false);
                ApplyGeometryRefreshData(existing, refresh);
                generatedPreview = refresh.PreviewGenerated;
                existing.CachedAt = DateTime.UtcNow;
                existing.FileModifiedAt = info.LastWriteTimeUtc;
                existing.FileSize = info.Length;
            }
        }

        await db.SaveChangesAsync();
        var shouldGenerateTags = false;

        if ((generateTagsOnScan || generateDescriptionsOnScan) && existing is not null)
        {
            tagsStale = generateTagsOnScan
                && TagGenerationService.NeedsRegeneration(existing, appConfig, configuredTagSchema);
            descriptionStale = generateDescriptionsOnScan
                && TagGenerationService.NeedsDescriptionRegeneration(existing, appConfig);
            expectedTagChecksum = generateTagsOnScan && configuredTagSchema.Count > 0
                ? TagGenerationService.ComputeGenerationChecksum(existing, appConfig, configuredTagSchema)
                : null;
            expectedDescriptionChecksum = generateDescriptionsOnScan
                ? TagGenerationService.ComputeDescriptionChecksum(existing, appConfig)
                : null;
        }

        shouldGenerateTags = (generateTagsOnScan || generateDescriptionsOnScan)
            && (tagsStale || descriptionStale);
        logger.LogDebug(
            "Single-model scan AI-generation decision for {FilePath}: shouldGenerate={ShouldGenerate}, tagGenerationEnabled={TagGenerationEnabled}, descriptionGenerationEnabled={DescriptionGenerationEnabled}, schemaCount={SchemaCount}, checksumChanged={ChecksumChanged}, hullStale={HullStale}, previewStale={PreviewStale}, tagsStale={TagsStale}, descriptionStale={DescriptionStale}, tagStatus={TagStatus}, tagJsonPresent={TagJsonPresent}, storedTagChecksum={StoredTagChecksum}, expectedTagChecksum={ExpectedTagChecksum}, descriptionPresent={DescriptionPresent}, storedDescriptionChecksum={StoredDescriptionChecksum}, expectedDescriptionChecksum={ExpectedDescriptionChecksum}",
            fullPath,
            shouldGenerateTags,
            generateTagsOnScan,
            generateDescriptionsOnScan,
            configuredTagSchema.Count,
            checksumChanged,
            hullStale,
            previewStale,
            tagsStale,
            descriptionStale,
            existing?.GeneratedTagsStatus,
            !string.IsNullOrWhiteSpace(existing?.GeneratedTagsJson),
            storedTagChecksum,
            expectedTagChecksum,
            !string.IsNullOrWhiteSpace(existing?.GeneratedDescription),
            storedDescriptionChecksum,
            expectedDescriptionChecksum);
        var generatedAiTags = false;
        var generatedAiDescription = false;
        string? aiGenerationReason = null;
        if (shouldGenerateTags && existing is not null)
        {
            cancellationToken.ThrowIfCancellationRequested();
            aiGenerationReason = isNewModel
                ? "new model preview"
                : generatedPreview
                    ? "preview updated"
                    : tagsStale && descriptionStale
                        ? "tags+description stale"
                        : tagsStale
                            ? "tags checksum stale"
                            : descriptionStale
                                ? "description checksum stale"
                                : null;
            var tagResult = await TryGenerateTagsOnScanAsync(existing.Id, generateTagsOnScan, generateDescriptionsOnScan);
            generatedAiTags = tagResult.GeneratedTags;
            generatedAiDescription = tagResult.GeneratedDescription;
        }

        if (progressReporter is not null)
        {
            await progressReporter.OnFileProcessedAsync(new IndexingFileResult(
                normalized,
                fileType,
                "processed",
                IsNew: isNewModel,
                WasUpdated: true,
                GeneratedPreview: generatedPreview,
                GeneratedHull: generatedHull,
                GeneratedAiTags: generatedAiTags,
                GeneratedAiDescription: generatedAiDescription,
                AiGenerationReason: generatedAiTags || generatedAiDescription ? aiGenerationReason : null,
                Message: "Indexed single model",
                DurationMs: singleFileTimer.Elapsed.TotalMilliseconds));
        }
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
        bool PreviewGenerated,
        string? ConvexHull,
        string? ConcaveHull,
        string? ConvexSansRaftHull,
        bool HullGenerated,
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
        bool PreviewGenerated,
        string? ConvexHull,
        string? ConcaveHull,
        string? ConvexSansRaftHull,
        bool HullGenerated,
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
        GeometryRefreshData? GeometryRefresh,
        bool FileMetadataOnlyRefresh,
        bool ForceTagGeneration,
        string? AiGenerationReason,
        double PreparationDurationMs = 0);

    private sealed record ExistingModelScanState(
        Guid Id,
        string Directory,
        string FileName,
        string Checksum,
        DateTime FileModifiedAt,
        string? ScanConfigChecksum,
        int? PreviewGenerationVersion,
        DateTime? PreviewGeneratedAt,
        string? GeneratedTagsChecksum,
        string? GeneratedTagsStatus,
        string? GeneratedTagsJson,
        DateTime? GeneratedTagsAt,
        string? GeneratedDescription,
        string? GeneratedDescriptionChecksum,
        DateTime? GeneratedDescriptionAt,
        string? PreviewImagePath,
        string? CalculatedModelName,
        string? CalculatedPartName,
        string? CalculatedCategory,
        string? CalculatedType,
        string? CalculatedMaterial,
        string? CalculatedCreator,
        string? CalculatedCollection,
        string? CalculatedSubcollection);

    private async Task<PreparedScanResult?> PrepareScanResultAsync(
        string file,
        string modelsPath,
        IReadOnlyDictionary<(string Directory, string FileName), ExistingModelScanState> existingModels,
        Dictionary<string, DirectoryConfig> directoryConfigs,
        float defaultRaftHeightMm,
        AppConfigDto appConfig,
        List<string> configuredTagSchema,
        bool generateTagsOnScan,
        bool generateDescriptionsOnScan)
    {
        var relativePath = Path.GetRelativePath(modelsPath, file).Replace('\\', '/');
        var fileName = Path.GetFileName(file);
        var directory = (Path.GetDirectoryName(relativePath) ?? "").Replace('\\', '/');
        var info = new FileInfo(file);
        var fileType = Path.GetExtension(file).TrimStart('.').ToLowerInvariant();

        if (existingModels.TryGetValue((directory, fileName), out var existingEntry))
        {
            var checksumEvaluation = await ResolveChecksumAsync(
                file,
                info.LastWriteTimeUtc,
                existingEntry.Checksum,
                existingEntry.FileModifiedAt);
            var currentChecksum = checksumEvaluation.Checksum;
            var expectedRaftHeightMm = ResolveRaftHeightMmForModel(
                file,
                directory,
                directoryConfigs,
                defaultRaftHeightMm);
            var hullMetadataMismatch = !IsNonGeometryType(fileType)
                && NeedsHullRegeneration(existingEntry.ScanConfigChecksum, expectedRaftHeightMm);
            var previewStale = !IsNonGeometryType(fileType)
                && NeedsPreviewRegeneration(existingEntry.PreviewGenerationVersion);
            var tagsStale = false;
            var descriptionStale = false;
            if (generateTagsOnScan || generateDescriptionsOnScan)
            {
                var projected = new CachedModel
                {
                    Id = existingEntry.Id,
                    Checksum = existingEntry.Checksum,
                    FileName = existingEntry.FileName,
                    Directory = existingEntry.Directory,
                    PreviewImagePath = existingEntry.PreviewImagePath,
                    PreviewGeneratedAt = existingEntry.PreviewGeneratedAt,
                    PreviewGenerationVersion = existingEntry.PreviewGenerationVersion,
                    CalculatedModelName = existingEntry.CalculatedModelName,
                    CalculatedPartName = existingEntry.CalculatedPartName,
                    CalculatedCategory = existingEntry.CalculatedCategory,
                    CalculatedType = existingEntry.CalculatedType,
                    CalculatedMaterial = existingEntry.CalculatedMaterial,
                    CalculatedCreator = existingEntry.CalculatedCreator,
                    CalculatedCollection = existingEntry.CalculatedCollection,
                    CalculatedSubcollection = existingEntry.CalculatedSubcollection,
                    GeneratedTagsChecksum = existingEntry.GeneratedTagsChecksum,
                    GeneratedTagsJson = existingEntry.GeneratedTagsJson,
                    GeneratedTagsStatus = existingEntry.GeneratedTagsStatus,
                    GeneratedTagsAt = existingEntry.GeneratedTagsAt,
                    GeneratedDescription = existingEntry.GeneratedDescription,
                    GeneratedDescriptionChecksum = existingEntry.GeneratedDescriptionChecksum,
                    GeneratedDescriptionAt = existingEntry.GeneratedDescriptionAt,
                };
                tagsStale = generateTagsOnScan
                    && TagGenerationService.NeedsRegeneration(projected, appConfig, configuredTagSchema);
                descriptionStale = generateDescriptionsOnScan
                    && TagGenerationService.NeedsDescriptionRegeneration(projected, appConfig);
            }

            if (currentChecksum == existingEntry.Checksum && !hullMetadataMismatch && !previewStale && !tagsStale && !descriptionStale)
            {
                if (checksumEvaluation.Recomputed)
                {
                    return new PreparedScanResult(
                        existingEntry.Id,
                        file,
                        relativePath,
                        directory,
                        fileName,
                        fileType,
                        info,
                        null,
                        null,
                        true,
                        false,
                        null);
                }

                return null;
            }

            if (currentChecksum != existingEntry.Checksum)
            {
                var data = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, currentChecksum, defaultRaftHeightMm);
                return new PreparedScanResult(
                    existingEntry.Id,
                    file,
                    relativePath,
                    directory,
                    fileName,
                    fileType,
                    info,
                    data,
                    null,
                    false,
                    false,
                    null);
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
                return new PreparedScanResult(existingEntry.Id, file, relativePath, directory, fileName, fileType, info, null, refresh, false, false, null);
            }

            if (tagsStale || descriptionStale)
            {
                var aiReason = tagsStale && descriptionStale
                    ? "tags+description stale"
                    : tagsStale
                        ? "tags checksum stale"
                        : "description checksum stale";
                return new PreparedScanResult(existingEntry.Id, file, relativePath, directory, fileName, fileType, info, null, null, false, true, aiReason);
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
            return new PreparedScanResult(existingEntry.Id, file, relativePath, directory, fileName, fileType, info, null, previewRefresh, false, false, null);
        }

        var newData = await ComputeFileDataAsync(file, fileType, directory, directoryConfigs, null, defaultRaftHeightMm);
        return new PreparedScanResult(
            null,
            file,
            relativePath,
            directory,
            fileName,
            fileType,
            info,
            newData,
            null,
            false,
            false,
            null);
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
        var previewGenerated = false;
        string? convexHull = null, concaveHull = null, convexSansRaftHull = null;
        var hullGenerated = false;
        float? dimensionXMm = null, dimensionYMm = null, dimensionZMm = null;
        float? sphereCentreX = null, sphereCentreY = null, sphereCentreZ = null, sphereRadius = null;

        if (!IsNonGeometryType(fileType))
        {
            geometry = await loaderService.LoadModelAsync(file, fileType);
            if (geometry is not null)
            {
                var previewResult = await previewService.GeneratePreviewWithStatusAsync(geometry, checksum);
                previewImagePath = previewResult.RelativePath;
                previewGenerated = previewResult.Generated;
            }

            if (geometry is not null)
            {
                (convexHull, concaveHull, convexSansRaftHull) =
                    await hullCalculationService.CalculateHullsAsync(geometry, raftHeightMm: raftHeightMm);
                hullGenerated = true;
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
            previewGenerated,
            convexHull,
            concaveHull,
            convexSansRaftHull,
            hullGenerated,
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
            return new GeometryRefreshData(includePreview, includeHulls, false, null, false, null, null, null, false, raftHeightMm);

        string? previewImagePath = null;
        var previewGenerated = false;
        if (includePreview)
        {
            var previewResult = await previewService.GeneratePreviewWithStatusAsync(geometry, checksum);
            previewImagePath = previewResult.RelativePath;
            previewGenerated = previewResult.Generated;
        }

        string? convexHull = null;
        string? concaveHull = null;
        string? convexSansRaftHull = null;
        var hullGenerated = false;
        if (includeHulls)
        {
            (convexHull, concaveHull, convexSansRaftHull) =
                await hullCalculationService.CalculateHullsAsync(geometry, raftHeightMm: raftHeightMm);
            hullGenerated = true;
        }

        return new GeometryRefreshData(
            includePreview,
            includeHulls,
            true,
            previewImagePath,
                previewGenerated,
            convexHull,
            concaveHull,
            convexSansRaftHull,
                hullGenerated,
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

    private static async Task<(string Checksum, bool Recomputed)> ResolveChecksumAsync(
        string filePath,
        DateTime observedFileModifiedAt,
        string? existingChecksum,
        DateTime? existingFileModifiedAt)
    {
        if (!string.IsNullOrWhiteSpace(existingChecksum)
            && existingFileModifiedAt.HasValue
            && existingFileModifiedAt.Value == observedFileModifiedAt)
        {
            return (existingChecksum, false);
        }

        var checksum = await ComputeChecksumAsync(filePath);
        return (checksum, true);
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

    private async Task<TagGenerationProgress> TryGenerateTagsOnScanAsync(
        Guid modelId,
        bool generateTagsOnScan,
        bool generateDescriptionsOnScan)
    {
        if (!generateTagsOnScan && !generateDescriptionsOnScan)
        {
            logger.LogDebug("Skipping AI generation for model {ModelId}: both toggles disabled", modelId);
            return new TagGenerationProgress(false, false, false, "AI generation disabled");
        }

        try
        {
            logger.LogDebug("Triggering tag generation during scan for model {ModelId}", modelId);
            var result = await tagGenerationService.GenerateForModelAsync(modelId, CancellationToken.None);
            logger.LogDebug("Tag generation finished during scan for model {ModelId}", modelId);
            var success = string.Equals(result?.Status, "success", StringComparison.OrdinalIgnoreCase)
                || string.Equals(result?.Status, "skipped", StringComparison.OrdinalIgnoreCase);
            return new TagGenerationProgress(
                success,
                string.Equals(result?.Status, "success", StringComparison.OrdinalIgnoreCase) && generateTagsOnScan,
                string.Equals(result?.Status, "success", StringComparison.OrdinalIgnoreCase) && generateDescriptionsOnScan,
                success ? null : result?.Error);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Tag generation failed during scan for model {ModelId}; scan will continue.", modelId);
            return new TagGenerationProgress(false, false, false, ex.Message);
        }
    }

    private sealed record TagGenerationProgress(
        bool Success,
        bool GeneratedTags,
        bool GeneratedDescription,
        string? Error);

}
