using System.Security.Cryptography;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using YamlDotNet.Serialization;
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
    ILogger<MetadataConfigService> logger,
    IDbContextFactory<ModelCacheContext> dbFactory,
    IConfiguration configuration)
{
    private int DirectoryBatchSize => configuration.GetValue("Indexing:DirectoryBatchSize", 1000);
    private const string ConfigFileName = "findamodel.yaml";
    private static readonly IDeserializer YamlDeserializer = new DeserializerBuilder().Build();
    private static readonly ISerializer YamlSerializer = new SerializerBuilder()
        .ConfigureDefaultValuesHandling(DefaultValuesHandling.OmitNull)
        .Build();

    private static readonly HashSet<string> KnownRuleTypes =
        new(StringComparer.OrdinalIgnoreCase) { "filename", "regex" };

    /// <summary>
    /// Validates each field rule entry; returns a map of fieldName → error message for any failures.
    /// </summary>
    private static Dictionary<string, string> ValidateRules(Dictionary<string, string> fieldRules)
    {
        var errors = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (fieldName, ruleYaml) in fieldRules)
        {
            if (string.IsNullOrWhiteSpace(ruleYaml)) continue;

            Dictionary<string, object>? ruleObj;
            try
            {
                ruleObj = YamlDeserializer.Deserialize<Dictionary<string, object>>(ruleYaml);
            }
            catch (Exception ex)
            {
                errors[fieldName] = $"Invalid YAML: {ex.Message}";
                continue;
            }

            if (ruleObj == null || ruleObj.Count == 0)
            {
                errors[fieldName] = "Invalid YAML: rule is empty";
                continue;
            }

            if (!ruleObj.TryGetValue("rule", out var ruleNameObj) || ruleNameObj == null)
            {
                errors[fieldName] = "Must include a \"rule:\" key (e.g. rule: filename)";
                continue;
            }

            var ruleName = ruleNameObj.ToString() ?? "";
            if (!KnownRuleTypes.Contains(ruleName))
                errors[fieldName] = $"Unknown rule type \"{ruleName}\". Valid types: {string.Join(", ", KnownRuleTypes)}";
        }
        return errors;
    }

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
            var errors = ValidateRules(req.FieldRules);
            if (errors.Count > 0)
                throw new ConfigValidationException(errors);
        }

        await WriteConfigFileAsync(rootPath, dirPath, req);

        var fullDirPath = GetFullDirPath(rootPath, dirPath);
        var configFilePath = Path.Combine(fullDirPath, ConfigFileName);
        var newHash = File.Exists(configFilePath) ? await ComputeFileHashAsync(configFilePath) : null;

        await using var db = await dbFactory.CreateDbContextAsync();
        var all = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);

        if (!all.TryGetValue(dirPath, out var record))
        {
            var parent = GetParentRecord(dirPath, all);
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

        var rawFields = File.Exists(configFilePath) ? await ParseConfigFileAsync(configFilePath) : null;
        ApplyRawFields(record, rawFields);
        record.LocalConfigFileHash = newHash;
        ResolveFields(record, all);
        record.UpdatedAt = DateTime.UtcNow;

        ResolveDescendants(dirPath, all);

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
            var affectedDirs = ExpandToDescendants(changedDirs, existing.Keys);
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
        var sorted = ExpandToAllAncestors(relativeDirectories)
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
            var affectedDirs = ExpandToDescendants(changedDirs, existing.Keys);
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
            var fullFilePath = BuildFullFilePath(modelsRootPath, model.Directory, model.FileName);
            var metadata = ModelMetadataHelper.Compute(fullFilePath, dirConfig);
            model.ApplyCalculatedMetadata(metadata);
        }
    }

    private static string BuildFullFilePath(string modelsRootPath, string directory, string fileName) =>
        string.IsNullOrEmpty(directory)
            ? Path.Combine(modelsRootPath, fileName)
            : Path.Combine(modelsRootPath, directory.Replace('/', Path.DirectorySeparatorChar), fileName);

    /// <summary>
    /// Expands a set of changed root directories to include all their descendants from allKnownDirs.
    /// </summary>
    private static List<string> ExpandToDescendants(HashSet<string> changedRoots, IEnumerable<string> allKnownDirs)
    {
        var result = new HashSet<string>(changedRoots, StringComparer.Ordinal);
        foreach (var dir in allKnownDirs)
        {
            foreach (var root in changedRoots)
            {
                if (root == "" || dir.StartsWith(root + "/", StringComparison.Ordinal))
                {
                    result.Add(dir);
                    break;
                }
            }
        }
        return result.ToList();
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
        var fullPath = GetFullDirPath(modelsRootPath, dirPath);
        var configFilePath = Path.Combine(fullPath, ConfigFileName);
        var configExists = File.Exists(configFilePath);
        var currentHash = configExists ? await ComputeFileHashAsync(configFilePath) : null;

        if (existing.TryGetValue(dirPath, out var record))
        {
            var changed = record.LocalConfigFileHash != currentHash;
            if (changed)
            {
                logger.LogInformation(
                    "Config file changed for directory '{Dir}': {Old} → {New}",
                    dirPath, record.LocalConfigFileHash ?? "(none)", currentHash ?? "(none)");
                ApplyRawFields(record, configExists ? await ParseConfigFileAsync(configFilePath) : null);
                record.LocalConfigFileHash = currentHash;
                record.UpdatedAt = DateTime.UtcNow;
            }
            if (changed || alwaysResolve)
                ResolveFields(record, existing);
            return changed;
        }

        var parent = GetParentRecord(dirPath, existing);
        var newRecord = new DirectoryConfig
        {
            Id = Guid.NewGuid(),
            DirectoryPath = dirPath,
            ParentId = parent?.Id,
            LocalConfigFileHash = currentHash,
            UpdatedAt = DateTime.UtcNow
        };
        ApplyRawFields(newRecord, configExists ? await ParseConfigFileAsync(configFilePath) : null);
        ResolveFields(newRecord, existing);
        db.DirectoryConfigs.Add(newRecord);
        existing[dirPath] = newRecord;
        return true;
    }

    // -------------------------------------------------------------------------
    // Shared: descendant re-resolution
    // -------------------------------------------------------------------------

    /// <summary>
    /// Re-resolves all descendants of <paramref name="parentPath"/> using the provided
    /// in-memory dictionary. Updates resolved fields and UpdatedAt in place.
    /// Does NOT call SaveChanges — callers are responsible for saving.
    /// </summary>
    private static void ResolveDescendants(string parentPath, Dictionary<string, DirectoryConfig> all)
    {
        var prefix = parentPath == "" ? "" : parentPath + "/";

        // Collect all descendants; sort ancestor-first so each child sees its parent already resolved
        var descendants = all.Values
            .Where(d => parentPath == ""
                ? d.DirectoryPath != ""
                : d.DirectoryPath.StartsWith(prefix, StringComparison.Ordinal))
            .OrderBy(d => d.DirectoryPath.Length)
            .ThenBy(d => d.DirectoryPath, StringComparer.Ordinal)
            .ToList();

        foreach (var desc in descendants)
        {
            ResolveFields(desc, all);
            desc.UpdatedAt = DateTime.UtcNow;
        }
    }

    // -------------------------------------------------------------------------
    // Private: YAML writing
    // -------------------------------------------------------------------------

    private async Task WriteConfigFileAsync(string rootPath, string dirPath, UpdateDirectoryConfigRequest req)
    {
        var fullDirPath = GetFullDirPath(rootPath, dirPath);
        var configPath = Path.Combine(fullDirPath, ConfigFileName);

        // Build ordered dictionary of fields that are explicitly set (non-null plain values)
        var data = new Dictionary<string, object>();
        if (req.ModelName != null) data["model_name"] = req.ModelName;
        if (req.Creator != null) data["creator"] = req.Creator;
        if (req.Collection != null) data["collection"] = req.Collection;
        if (req.Subcollection != null) data["subcollection"] = req.Subcollection;
        if (req.Category != null) data["category"] = req.Category;
        if (req.Type != null) data["type"] = req.Type;
        if (req.Material != null) data["material"] = req.Material;
        if (req.Supported.HasValue) data["supported"] = req.Supported.Value;

        if (req.FieldRules != null)
        {
            // Explicit rule management: write exactly the provided rules (plain values take precedence).
            // Fields not in FieldRules get no rule written — this clears any previously saved rules.
            foreach (var (fieldName, ruleYaml) in req.FieldRules)
            {
                if (data.ContainsKey(fieldName)) continue; // plain value wins
                if (string.IsNullOrWhiteSpace(ruleYaml)) continue;
                try
                {
                    var ruleObj = YamlDeserializer.Deserialize<Dictionary<string, object>>(ruleYaml);
                    if (ruleObj != null && ruleObj.ContainsKey("rule"))
                        data[fieldName] = ruleObj;
                }
                catch (Exception ex)
                {
                    logger.LogWarning(ex, "Invalid rule YAML for field '{Field}'; skipping", fieldName);
                }
            }
        }
        else
        {
            // Legacy path: preserve any rule definitions that exist in the current YAML file
            // for fields not being overridden by a plain value.
            var existingRules = new Dictionary<string, object>(StringComparer.OrdinalIgnoreCase);
            if (File.Exists(configPath))
            {
                try
                {
                    var existingYaml = await File.ReadAllTextAsync(configPath);
                    var existing = YamlDeserializer.Deserialize<Dictionary<string, object>>(existingYaml);
                    if (existing != null)
                    {
                        var ruleFieldNames = MetadataFieldRegistry.Keys.ToArray();
                        foreach (var kvp in existing)
                        {
                            var fieldName = ruleFieldNames.FirstOrDefault(f =>
                                string.Equals(f, kvp.Key, StringComparison.OrdinalIgnoreCase));
                            if (fieldName == null) continue;

                            Dictionary<string, object>? ruleObj = kvp.Value switch
                            {
                                Dictionary<string, object> d => d,
                                Dictionary<object, object> d2 => d2
                                    .Where(kv => kv.Key != null)
                                    .ToDictionary(kv => kv.Key.ToString()!, kv => kv.Value),
                                _ => null
                            };

                            if (ruleObj != null && ruleObj.ContainsKey("rule"))
                                existingRules[fieldName] = ruleObj;
                        }
                    }
                }
                catch (Exception ex)
                {
                    logger.LogWarning(ex, "Could not read existing rules from '{ConfigPath}'; rules may be lost", configPath);
                }
            }

            foreach (var (field, ruleObj) in existingRules)
                if (!data.ContainsKey(field))
                    data[field] = ruleObj;
        }

        if (data.Count == 0)
        {
            if (File.Exists(configPath)) File.Delete(configPath);
            return;
        }

        var yaml = YamlSerializer.Serialize(data);
        await File.WriteAllTextAsync(configPath, yaml);
    }

    // -------------------------------------------------------------------------
    // Private helpers
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
                Category = record.RawCategory,
                Type = record.RawType,
                Material = record.RawMaterial,
                Supported = record.RawSupported,
                ModelName = record.RawModelName,
            }
            : new ConfigFieldsDto();

        var parentPath = GetParentPath(dirPath);
        ConfigFieldsDto? parentResolved = null;
        DirectoryConfig? parentRecord = null;
        if (parentPath != null && all.TryGetValue(parentPath, out parentRecord))
            parentResolved = new ConfigFieldsDto
            {
                Creator = parentRecord.Creator,
                Collection = parentRecord.Collection,
                Subcollection = parentRecord.Subcollection,
                Category = parentRecord.Category,
                Type = parentRecord.Type,
                Material = parentRecord.Material,
                Supported = parentRecord.Supported,
                ModelName = parentRecord.ModelName,
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
                    // Serialize only the inner rule properties (without the outer field key wrapper)
                    var innerObj = ruleEl.EnumerateObject()
                        .ToDictionary(p => p.Name, p => JsonElementToYamlObject(p.Value));
                    localRuleContents[field.ToLowerInvariant()] = YamlSerializer.Serialize(innerObj).TrimEnd();
                }
            }
        }

        // Compute parent resolved rules (inherited rules from parent directory)
        Dictionary<string, string>? parentResolvedRules = null;
        if (parentRecord != null)
        {
            var parentRules = RuleRegistry.DeserializeRules(parentRecord.ResolvedRulesYaml);
            if (parentRules.Count > 0)
            {
                parentResolvedRules = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                foreach (var (field, ruleEl) in parentRules)
                {
                    // Skip any rules that are overridden locally
                    if (record?.RawRulesYaml != null)
                    {
                        var localRules = RuleRegistry.DeserializeRules(record.RawRulesYaml);
                        if (localRules.ContainsKey(field)) continue;
                    }
                    // Serialize only the inner rule properties
                    var innerObj = ruleEl.EnumerateObject()
                        .ToDictionary(p => p.Name, p => JsonElementToYamlObject(p.Value));
                    parentResolvedRules[field.ToLowerInvariant()] = YamlSerializer.Serialize(innerObj).TrimEnd();
                }
            }
        }

        return new DirectoryConfigDetailDto(dirPath, localValues, parentResolved, parentPath, localRuleFields, localRuleContents, parentResolvedRules);
    }

    private static string GetFullDirPath(string rootPath, string dirPath) =>
        dirPath == ""
            ? rootPath
            : Path.Combine(rootPath, dirPath.Replace('/', Path.DirectorySeparatorChar));

    private static string? GetParentPath(string dirPath)
    {
        if (dirPath == "") return null;
        var lastSlash = dirPath.LastIndexOf('/');
        return lastSlash < 0 ? "" : dirPath[..lastSlash];
    }

    /// <summary>
    /// Expands a set of relative directory paths to include all ancestor paths.
    /// E.g. "Fantasy/Elite" → {"", "Fantasy", "Fantasy/Elite"}.
    /// Root ("") is always included.
    /// </summary>
    private static HashSet<string> ExpandToAllAncestors(IEnumerable<string> paths)
    {
        var result = new HashSet<string>(StringComparer.Ordinal) { "" };
        foreach (var path in paths)
        {
            result.Add(path);
            var parts = path.Split('/', StringSplitOptions.RemoveEmptyEntries);
            for (int i = 1; i < parts.Length; i++)
                result.Add(string.Join('/', parts[..i]));
        }
        return result;
    }

    /// <summary>
    /// Returns the parent DirectoryConfig record for the given path.
    /// "" → null. "a/b/c" → "a/b". "a" → "".
    /// </summary>
    private static DirectoryConfig? GetParentRecord(string dirPath, Dictionary<string, DirectoryConfig> existing)
    {
        var parentPath = GetParentPath(dirPath);
        if (parentPath == null) return null;
        return existing.TryGetValue(parentPath, out var parent) ? parent : null;
    }

    /// <summary>
    /// Resolves all metadata fields by walking up the ancestor chain.
    /// Uses Raw* fields only (never resolved fields) to prevent cascading errors.
    /// The record's own Raw values take priority; the first non-null value or rule found wins.
    /// A field resolved to a rule is stored in ResolvedRulesJson; plain values use the existing columns.
    /// </summary>
    private static void ResolveFields(DirectoryConfig record, Dictionary<string, DirectoryConfig> allRecords)
    {
        var resolvedValues = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);
        var resolvedRules = new Dictionary<string, JsonElement>(StringComparer.OrdinalIgnoreCase);
        var claimedFields = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var field in MetadataFieldRegistry.Definitions)
        {
            var rawValue = field.GetRawValue(record);
            resolvedValues[field.Key] = rawValue;
            if (rawValue != null)
                claimedFields.Add(field.Key);
        }

        var localRules = RuleRegistry.DeserializeRules(record.RawRulesYaml);
        foreach (var (fieldKey, ruleEl) in localRules)
        {
            if (!MetadataFieldRegistry.TryGet(fieldKey, out var fieldDefinition))
                continue;
            if (!claimedFields.Add(fieldDefinition.Key))
                continue;
            resolvedRules[fieldDefinition.Key] = ruleEl;
        }

        var current = GetParentRecord(record.DirectoryPath, allRecords);
        while (current != null && claimedFields.Count < MetadataFieldRegistry.Definitions.Length)
        {
            var parentRules = RuleRegistry.DeserializeRules(current.RawRulesYaml);
            foreach (var field in MetadataFieldRegistry.Definitions)
            {
                if (claimedFields.Contains(field.Key))
                    continue;

                var rawValue = field.GetRawValue(current);
                if (rawValue != null)
                {
                    resolvedValues[field.Key] = rawValue;
                    claimedFields.Add(field.Key);
                    continue;
                }

                if (parentRules.TryGetValue(field.Key, out var ruleEl))
                {
                    resolvedRules[field.Key] = ruleEl;
                    claimedFields.Add(field.Key);
                }
            }

            current = GetParentRecord(current.DirectoryPath, allRecords);
        }

        foreach (var field in MetadataFieldRegistry.Definitions)
            field.SetResolvedValue(record, resolvedValues.GetValueOrDefault(field.Key));

        record.ResolvedRulesYaml = resolvedRules.Count > 0
            ? SerializeRulesToYaml(resolvedRules)
            : null;
    }

    private static void ApplyRawFields(DirectoryConfig record, RawConfigFields? fields)
    {
        record.RawCreator = fields?.Creator;
        record.RawCollection = fields?.Collection;
        record.RawSubcollection = fields?.Subcollection;
        record.RawCategory = fields?.Category;
        record.RawType = fields?.Type;
        record.RawMaterial = fields?.Material;
        record.RawSupported = fields?.Supported;
        record.RawModelName = fields?.ModelName;
        record.RawRulesYaml = fields?.RulesYaml;
    }

    private static async Task<string> ComputeFileHashAsync(string filePath)
    {
        await using var stream = File.OpenRead(filePath);
        var hash = await SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLower();
    }

    private async Task<RawConfigFields?> ParseConfigFileAsync(string filePath)
    {
        try
        {
            using var reader = new StreamReader(filePath);
            var yaml = await reader.ReadToEndAsync();
            var parsed = YamlDeserializer.Deserialize<Dictionary<string, object>>(yaml) ?? new Dictionary<string, object>();

            return new RawConfigFields(
                Creator: TryGetString(parsed, "creator"),
                Collection: TryGetString(parsed, "collection"),
                Subcollection: TryGetString(parsed, "subcollection"),
                Category: MetadataFieldRegistry.ValidateEnumValue("category", TryGetString(parsed, "category")),
                Type: MetadataFieldRegistry.ValidateEnumValue("type", TryGetString(parsed, "type")),
                Material: MetadataFieldRegistry.ValidateEnumValue("material", TryGetString(parsed, "material")),
                Supported: TryGetBool(parsed, "supported"),
                ModelName: TryGetString(parsed, "model_name"),
                RulesYaml: ExtractRulesYaml(parsed)
            );
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to parse config file '{FilePath}'; treating as empty", filePath);
            return null;
        }
    }

    /// <summary>
    /// Extracts rule definitions from YAML-parsed data and serializes them back to YAML.
    /// Fields whose value is a mapping containing a "rule" key are treated as rules.
    /// Returns a YAML string or null if no rules found.
    /// </summary>
    private static string? ExtractRulesYaml(Dictionary<string, object> data)
    {
        var ruleFieldNames = MetadataFieldRegistry.Keys.ToArray();
        var rules = new Dictionary<string, object>();

        foreach (var kvp in data)
        {
            var fieldName = ruleFieldNames.FirstOrDefault(f =>
                string.Equals(f, kvp.Key, StringComparison.OrdinalIgnoreCase));
            if (fieldName == null) continue;

            Dictionary<string, object>? ruleObj = kvp.Value switch
            {
                Dictionary<string, object> d => d,
                Dictionary<object, object> d2 => d2
                    .Where(kv => kv.Key != null)
                    .ToDictionary(kv => kv.Key.ToString()!, kv => kv.Value),
                _ => null
            };

            if (ruleObj == null || !ruleObj.ContainsKey("rule")) continue;
            rules[fieldName] = ruleObj;
        }

        return rules.Count > 0 ? YamlSerializer.Serialize(rules) : null;
    }

    /// <summary>
    /// Serializes a resolved rules dictionary (JsonElement values) to YAML for storage.
    /// </summary>
    private static string SerializeRulesToYaml(Dictionary<string, JsonElement> rules)
    {
        var plain = rules.ToDictionary(kvp => kvp.Key, kvp => JsonElementToYamlObject(kvp.Value));
        return YamlSerializer.Serialize(plain);
    }

    private static object? JsonElementToYamlObject(JsonElement el) => el.ValueKind switch
    {
        JsonValueKind.Object => el.EnumerateObject()
            .ToDictionary(p => p.Name, p => JsonElementToYamlObject(p.Value)),
        JsonValueKind.Array => el.EnumerateArray().Select(JsonElementToYamlObject).ToList<object?>(),
        JsonValueKind.String => (object?)el.GetString(),
        JsonValueKind.Number when el.TryGetInt64(out var i) => (object?)i,
        JsonValueKind.Number => (object?)el.GetDouble(),
        JsonValueKind.True => (object?)true,
        JsonValueKind.False => (object?)false,
        _ => null
    };

    private static string? TryGetString(Dictionary<string, object> data, string propertyName)
    {
        // Case-insensitive property search
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase)) continue;

            // Skip mappings — they represent rule definitions, not plain string values
            if (kvp.Value is Dictionary<object, object> || kvp.Value is Dictionary<string, object>) return null;

            var value = kvp.Value?.ToString();
            return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
        }
        return null;
    }

    private static bool? TryGetBool(Dictionary<string, object> data, string propertyName)
    {
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase)) continue;

            // Skip mappings — they represent rule definitions
            if (kvp.Value is Dictionary<object, object> || kvp.Value is Dictionary<string, object>) return null;

            if (kvp.Value is bool b) return b;
            if (kvp.Value is string s && bool.TryParse(s, out var parsed)) return parsed;
        }
        return null;
    }

    private sealed record RawConfigFields(string? Creator, string? Collection, string? Subcollection, string? Category, string? Type, string? Material, bool? Supported, string? ModelName = null, string? RulesYaml = null);
}
