using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services.Rules;

namespace findamodel.Services;

/// <summary>Thrown when one or more rule YAML entries in a config update fail validation.</summary>
public class ConfigValidationException(Dictionary<string, string> fieldErrors) : Exception("Config validation failed")
{
    public Dictionary<string, string> FieldErrors { get; } = fieldErrors;
}

public class MetadataConfigService(
    IConfiguration config,
    ILoggerFactory loggerFactory,
    IDbContextFactory<ModelCacheContext> dbFactory,
    DirectoryConfigReader configReader)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Indexing);
    private int DirectoryBatchSize => config.GetValue("Indexing:DirectoryBatchSize", 1000);

    // -------------------------------------------------------------------------
    // Public API — used by ExplorerController
    // -------------------------------------------------------------------------

    /// <summary>
    /// Scans the full models tree and syncs DirectoryConfig records from findamodel.yaml files.
    /// Does not index any model files. Safe to call on startup.
    /// </summary>
    public async Task SyncDirectoryConfigsAsync()
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

        var count = await SyncDirectoryConfigsStreamingAsync(
            modelsPath, EnumerateDepthFirst(modelsPath));
        logger.LogInformation("Directory config sync complete: {Count} directories processed.", count);
    }

    /// <summary>
    /// Yields the root ("") followed by all subdirectories in depth-first pre-order,
    /// so every parent is emitted before its children.
    /// </summary>
    private static IEnumerable<string> EnumerateDepthFirst(string rootPath)
    {
        yield return "";

        var stack = new Stack<string>();
        foreach (var d in Directory.GetDirectories(rootPath).OrderByDescending(d => d, StringComparer.Ordinal))
            stack.Push(d);

        while (stack.Count > 0)
        {
            var current = stack.Pop();
            yield return Path.GetRelativePath(rootPath, current).Replace('\\', '/');
            try
            {
                foreach (var d in Directory.GetDirectories(current).OrderByDescending(d => d, StringComparer.Ordinal))
                    stack.Push(d);
            }
            catch (UnauthorizedAccessException) { }
        }
    }

    /// <summary>
    /// Returns the config detail for a directory: local raw values and the parent's
    /// resolved values (so the UI can show inherited placeholders per field).
    /// </summary>
    public async Task<DirectoryConfigDetailDto> GetDirectoryConfigDetailAsync(string dirPath)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var all = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);

        all.TryGetValue(dirPath, out var record);
        return BuildDetailDto(dirPath, record, all);
    }

    /// <summary>
    /// Updates the findamodel.yaml for a directory, writes the new raw values to the DB,
    /// re-resolves this directory's inherited fields, and propagates changes to all descendants.
    /// Returns the updated config detail.
    /// </summary>
    public async Task<DirectoryConfigDetailDto> UpdateDirectoryConfigAsync(
        string rootPath, string dirPath, UpdateDirectoryConfigRequest req)
    {
        if (req.FieldRules != null)
        {
            var errors = RuleValidator.ValidateRules(req.FieldRules);
            if (errors.Count > 0)
                throw new ConfigValidationException(errors);
        }

        await configReader.WriteConfigFileAsync(rootPath, dirPath, req);

        var fullDirPath = ConfigInheritanceResolver.GetFullDirPath(rootPath, dirPath);
        var configFilePath = Path.Combine(fullDirPath, DirectoryConfigReader.ConfigFileName);
        var newHash = File.Exists(configFilePath) ? await DirectoryConfigReader.ComputeFileHashAsync(configFilePath) : null;

        await using var db = await dbFactory.CreateDbContextAsync();
        var all = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);

        if (!all.TryGetValue(dirPath, out var record))
        {
            var parent = ConfigInheritanceResolver.GetParentRecord(dirPath, all);
            record = new DirectoryConfig
            {
                Id = Guid.NewGuid(),
                DirectoryPath = dirPath,
                ParentId = parent?.Id,
                UpdatedAt = DateTime.UtcNow
            };
            db.DirectoryConfigs.Add(record);
            all[dirPath] = record;
        }

        var rawFields = File.Exists(configFilePath) ? await configReader.ParseConfigFileAsync(configFilePath) : null;
        ConfigInheritanceResolver.ApplyRawFields(record, rawFields);
        record.LocalConfigFileHash = newHash;
        ConfigInheritanceResolver.ResolveFields(record, all);
        record.UpdatedAt = DateTime.UtcNow;

        ConfigInheritanceResolver.ResolveDescendants(dirPath, all);

        var affectedDirs = dirPath == ""
            ? all.Keys.ToList()
            : all.Keys
                .Where(k => k == dirPath || k.StartsWith(dirPath + "/", StringComparison.Ordinal))
                .ToList();
        await RecomputeModelCachesForDirectoriesAsync(db, rootPath, affectedDirs, all);

        await db.SaveChangesAsync();

        return BuildDetailDto(dirPath, record, all);
    }

    // -------------------------------------------------------------------------
    // Scan-time API — called by ModelService
    // -------------------------------------------------------------------------

    /// <summary>
    /// Syncs DirectoryConfig records by processing directories in streaming order.
    /// Directories must be in depth-first pre-order (parents before children) so that
    /// inheritance resolves correctly in a single pass — no ResolveDescendants needed.
    /// Returns the count of directories processed.
    /// </summary>
    public async Task<int> SyncDirectoryConfigsStreamingAsync(
        string modelsRootPath,
        IEnumerable<string> directories)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var existing = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);

        var changedDirs = new HashSet<string>(StringComparer.Ordinal);
        var count = 0;
        foreach (var dirPath in directories)
        {
            // alwaysResolve=true: DFS pre-order guarantees parent is current, so re-resolving
            // every dir propagates any upstream parent changes in one pass.
            bool changed = await SyncSingleDirectoryConfigAsync(db, modelsRootPath, dirPath, existing, alwaysResolve: true);
            if (changed) changedDirs.Add(dirPath);
            count++;

            if (count % DirectoryBatchSize == 0)
            {
                logger.LogInformation("SyncDirectoryConfigsStreamingAsync: processed {Count} directories so far...", count);
                await db.SaveChangesAsync();
            }
        }

        await db.SaveChangesAsync();

        if (changedDirs.Count > 0)
        {
            var affectedDirs = ConfigInheritanceResolver.ExpandToDescendants(changedDirs, existing.Keys);
            await RecomputeModelCachesForDirectoriesAsync(db, modelsRootPath, affectedDirs, existing);
            await db.SaveChangesAsync();
        }

        return count;
    }

    /// <summary>
    /// Ensures DirectoryConfig records exist for every directory in the given set (plus all
    /// ancestor directories up to root). Processes root-first so parent records always exist
    /// before children. Returns a dictionary keyed by DirectoryPath for O(1) lookup.
    /// </summary>
    public async Task<Dictionary<string, DirectoryConfig>> EnsureDirectoryConfigsAsync(
        string modelsRootPath,
        IEnumerable<string> relativeDirectories)
    {
        // Process root-first (shorter paths first; "" always comes first)
        var sorted = ConfigInheritanceResolver.ExpandToAllAncestors(relativeDirectories)
            .OrderBy(p => p.Length).ThenBy(p => p).ToList();

        await using var db = await dbFactory.CreateDbContextAsync();
        var existing = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);
        var changedDirs = new HashSet<string>(StringComparer.Ordinal);

        foreach (var dirPath in sorted)
        {
            // alwaysResolve=true: sorted is root-first, so each parent is already re-resolved
            // in `existing` before its children are processed. No need to cascade into sibling
            // trees via ResolveDescendants — only the paths we're actually scanning matter here.
            bool changed = await SyncSingleDirectoryConfigAsync(db, modelsRootPath, dirPath, existing, alwaysResolve: true);
            if (changed) changedDirs.Add(dirPath);
        }

        await db.SaveChangesAsync();

        if (changedDirs.Count > 0)
        {
            var affectedDirs = ConfigInheritanceResolver.ExpandToDescendants(changedDirs, existing.Keys);
            await RecomputeModelCachesForDirectoriesAsync(db, modelsRootPath, affectedDirs, existing);
            await db.SaveChangesAsync();
        }

        return await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);
    }

    // -------------------------------------------------------------------------
    // Private: model cache recomputation
    // -------------------------------------------------------------------------

    /// <summary>
    /// Recomputes Calculated* fields on CachedModel records for all models in the given
    /// set of directories, using the in-memory allConfigs dictionary for DirectoryConfig lookup.
    /// Does NOT call SaveChanges — callers are responsible.
    /// </summary>
    private static async Task RecomputeModelCachesForDirectoriesAsync(
        ModelCacheContext db,
        string modelsRootPath,
        IEnumerable<string> affectedDirs,
        Dictionary<string, DirectoryConfig> allConfigs)
    {
        var dirList = affectedDirs is List<string> l ? l : affectedDirs.ToList();
        if (dirList.Count == 0) return;

        var models = await db.Models
            .Where(m => dirList.Contains(m.Directory))
            .ToListAsync();

        foreach (var model in models)
        {
            allConfigs.TryGetValue(model.Directory, out var dirConfig);
            var fullFilePath = ConfigInheritanceResolver.BuildFullFilePath(modelsRootPath, model.Directory, model.FileName);
            var metadata = ModelMetadataHelper.Compute(fullFilePath, dirConfig);
            model.ApplyCalculatedMetadata(metadata);
        }
    }

    // -------------------------------------------------------------------------
    // Private: single-directory config sync
    // -------------------------------------------------------------------------

    /// <summary>
    /// Syncs a single directory's config record. If the config file hash changed (or the record
    /// is new), parses the file, applies raw fields, and resolves the record.
    /// When <paramref name="alwaysResolve"/> is true, resolves even when unchanged (needed when
    /// a parent's resolved values may have changed upstream).
    /// Returns true if the record was created or its hash changed.
    /// Does NOT call SaveChanges or ResolveDescendants — callers are responsible.
    /// </summary>
    private async Task<bool> SyncSingleDirectoryConfigAsync(
        ModelCacheContext db,
        string modelsRootPath,
        string dirPath,
        Dictionary<string, DirectoryConfig> existing,
        bool alwaysResolve)
    {
        var fullPath = ConfigInheritanceResolver.GetFullDirPath(modelsRootPath, dirPath);
        var configFilePath = Path.Combine(fullPath, DirectoryConfigReader.ConfigFileName);
        var configExists = File.Exists(configFilePath);
        var currentHash = configExists ? await DirectoryConfigReader.ComputeFileHashAsync(configFilePath) : null;

        if (existing.TryGetValue(dirPath, out var record))
        {
            var changed = record.LocalConfigFileHash != currentHash;
            if (changed)
            {
                logger.LogInformation(
                    "Config file changed for directory '{Dir}': {Old} → {New}",
                    dirPath, record.LocalConfigFileHash ?? "(none)", currentHash ?? "(none)");
                ConfigInheritanceResolver.ApplyRawFields(record, configExists ? await configReader.ParseConfigFileAsync(configFilePath) : null);
                record.LocalConfigFileHash = currentHash;
                record.UpdatedAt = DateTime.UtcNow;
            }
            if (changed || alwaysResolve)
                ConfigInheritanceResolver.ResolveFields(record, existing);
            return changed;
        }

        var parent = ConfigInheritanceResolver.GetParentRecord(dirPath, existing);
        var newRecord = new DirectoryConfig
        {
            Id = Guid.NewGuid(),
            DirectoryPath = dirPath,
            ParentId = parent?.Id,
            LocalConfigFileHash = currentHash,
            UpdatedAt = DateTime.UtcNow
        };
        ConfigInheritanceResolver.ApplyRawFields(newRecord, configExists ? await configReader.ParseConfigFileAsync(configFilePath) : null);
        ConfigInheritanceResolver.ResolveFields(newRecord, existing);
        db.DirectoryConfigs.Add(newRecord);
        existing[dirPath] = newRecord;
        return true;
    }

    // -------------------------------------------------------------------------
    // Private: DTO building
    // -------------------------------------------------------------------------

    private static DirectoryConfigDetailDto BuildDetailDto(
        string dirPath,
        DirectoryConfig? record,
        Dictionary<string, DirectoryConfig> all)
    {
        var localValues = record != null
            ? new ConfigFieldsDto
            {
                Creator = record.RawCreator,
                Collection = record.RawCollection,
                Subcollection = record.RawSubcollection,
                Tags = TagListHelper.FromJson(record.RawTagsJson),
                Category = record.RawCategory,
                Type = record.RawType,
                Material = record.RawMaterial,
                Supported = record.RawSupported,
                RaftHeightMm = record.RawRaftHeightMm,
                ModelName = record.RawModelName,
                PartName = record.RawPartName,
            }
            : new ConfigFieldsDto();

        var parentPath = ConfigInheritanceResolver.GetParentPath(dirPath);
        ConfigFieldsDto? parentResolved = null;
        DirectoryConfig? parentRecord = null;
        if (parentPath != null && all.TryGetValue(parentPath, out parentRecord))
            parentResolved = new ConfigFieldsDto
            {
                Creator = parentRecord.Creator,
                Collection = parentRecord.Collection,
                Subcollection = parentRecord.Subcollection,
                Tags = TagListHelper.FromJson(parentRecord.TagsJson),
                Category = parentRecord.Category,
                Type = parentRecord.Type,
                Material = parentRecord.Material,
                Supported = parentRecord.Supported,
                RaftHeightMm = parentRecord.RaftHeightMm,
                ModelName = parentRecord.ModelName,
                PartName = parentRecord.PartName,
            };

        HashSet<string>? localRuleFields = null;
        Dictionary<string, string>? localRuleContents = null;

        if (record?.RawRulesYaml != null)
        {
            var rules = RuleRegistry.DeserializeRules(record.RawRulesYaml);
            if (rules.Count > 0)
            {
                localRuleFields = rules.Keys.Select(k => k.ToLowerInvariant()).ToHashSet();
                localRuleContents = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                foreach (var (field, ruleEl) in rules)
                {
                    var innerObj = ruleEl.EnumerateObject()
                        .ToDictionary(p => p.Name, p => DirectoryConfigReader.JsonElementToYamlObject(p.Value));
                    localRuleContents[field.ToLowerInvariant()] = DirectoryConfigReader.YamlSerializer.Serialize(innerObj).TrimEnd();
                }
            }
        }

        Dictionary<string, string>? parentResolvedRules = null;
        if (parentRecord != null)
        {
            var parentRules = RuleRegistry.DeserializeRules(parentRecord.ResolvedRulesYaml);
            if (parentRules.Count > 0)
            {
                parentResolvedRules = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                foreach (var (field, ruleEl) in parentRules)
                {
                    if (record?.RawRulesYaml != null)
                    {
                        var localRules = RuleRegistry.DeserializeRules(record.RawRulesYaml);
                        if (localRules.ContainsKey(field)) continue;
                    }
                    var innerObj = ruleEl.EnumerateObject()
                        .ToDictionary(p => p.Name, p => DirectoryConfigReader.JsonElementToYamlObject(p.Value));
                    parentResolvedRules[field.ToLowerInvariant()] = DirectoryConfigReader.YamlSerializer.Serialize(innerObj).TrimEnd();
                }
            }
        }

        return new DirectoryConfigDetailDto(dirPath, localValues, parentResolved, parentPath, localRuleFields, localRuleContents, parentResolvedRules);
    }
}
