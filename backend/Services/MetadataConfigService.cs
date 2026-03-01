using System.Security.Cryptography;
using Microsoft.EntityFrameworkCore;
using YamlDotNet.Serialization;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class MetadataConfigService(
    ILogger<MetadataConfigService> logger,
    IDbContextFactory<ModelCacheContext> dbFactory)
{
    private const string ConfigFileName = "findamodel.yaml";
    private static readonly IDeserializer YamlDeserializer = new DeserializerBuilder().Build();
    private static readonly ISerializer YamlSerializer = new SerializerBuilder()
        .ConfigureDefaultValuesHandling(DefaultValuesHandling.OmitNull)
        .Build();

    // -------------------------------------------------------------------------
    // Public API — used by ExplorerController
    // -------------------------------------------------------------------------

    /// <summary>
    /// Returns the config detail for a directory: local raw values and the parent's
    /// resolved values (so the UI can show inherited placeholders per field).
    /// </summary>
    public async Task<DirectoryConfigDetailDto> GetDirectoryConfigDetailAsync(string dirPath)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var all = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);

        all.TryGetValue(dirPath, out var record);

        var localValues = record != null
            ? new ConfigFieldsDto(record.RawAuthor, record.RawCollection, record.RawSubcollection,
                                  record.RawCategory, record.RawType, record.RawSupported)
            : new ConfigFieldsDto(null, null, null, null, null, null);

        var parentPath = GetParentPath(dirPath);
        ConfigFieldsDto? parentResolved = null;
        if (parentPath != null && all.TryGetValue(parentPath, out var parentRecord))
        {
            parentResolved = new ConfigFieldsDto(parentRecord.Author, parentRecord.Collection,
                parentRecord.Subcollection, parentRecord.Category, parentRecord.Type, parentRecord.Supported);
        }

        return new DirectoryConfigDetailDto(dirPath, localValues, parentResolved, parentPath);
    }

    /// <summary>
    /// Updates the findamodel.yaml for a directory, writes the new raw values to the DB,
    /// re-resolves this directory's inherited fields, and propagates changes to all descendants.
    /// Returns the updated config detail.
    /// </summary>
    public async Task<DirectoryConfigDetailDto> UpdateDirectoryConfigAsync(
        string rootPath, string dirPath, UpdateDirectoryConfigRequest req)
    {
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

        var rawFields = new RawConfigFields(req.Author, req.Collection, req.Subcollection,
                                            req.Category, req.Type, req.Supported);
        ApplyRawFields(record, rawFields);
        record.LocalConfigFileHash = newHash;
        ResolveFields(record, all);
        record.UpdatedAt = DateTime.UtcNow;

        ResolveDescendants(dirPath, all);

        await db.SaveChangesAsync();

        var parentPath = GetParentPath(dirPath);
        ConfigFieldsDto? parentResolved = null;
        if (parentPath != null && all.TryGetValue(parentPath, out var parentRecord))
        {
            parentResolved = new ConfigFieldsDto(parentRecord.Author, parentRecord.Collection,
                parentRecord.Subcollection, parentRecord.Category, parentRecord.Type, parentRecord.Supported);
        }

        return new DirectoryConfigDetailDto(
            dirPath,
            new ConfigFieldsDto(record.RawAuthor, record.RawCollection, record.RawSubcollection,
                                 record.RawCategory, record.RawType, record.RawSupported),
            parentResolved,
            parentPath);
    }

    // -------------------------------------------------------------------------
    // Scan-time API — called by ModelService
    // -------------------------------------------------------------------------

    /// <summary>
    /// Ensures DirectoryConfig records exist for every directory in the given set (plus all
    /// ancestor directories up to root). Processes root-first so parent records always exist
    /// before children. Returns a dictionary keyed by DirectoryPath for O(1) lookup.
    /// </summary>
    public async Task<Dictionary<string, DirectoryConfig>> EnsureDirectoryConfigsAsync(
        string modelsRootPath,
        IEnumerable<string> relativeDirectories)
    {
        var allPaths = ExpandToAllAncestors(relativeDirectories);

        // Process root-first (shorter paths first; "" always comes first)
        var sorted = allPaths.OrderBy(p => p.Length).ThenBy(p => p).ToList();

        await using var db = await dbFactory.CreateDbContextAsync();

        // Load all existing records in one query
        var existing = await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);

        foreach (var dirPath in sorted)
        {
            var fullPath = GetFullDirPath(modelsRootPath, dirPath);
            var configFilePath = Path.Combine(fullPath, ConfigFileName);
            var configExists = File.Exists(configFilePath);

            var currentHash = configExists ? await ComputeFileHashAsync(configFilePath) : null;
            var rawFields = configExists ? await ParseConfigFileAsync(configFilePath) : null;

            if (existing.TryGetValue(dirPath, out var record))
            {
                // Record exists — check if the config file hash has changed
                if (record.LocalConfigFileHash != currentHash)
                {
                    logger.LogInformation(
                        "Config file changed for directory '{Dir}': {Old} → {New}",
                        dirPath,
                        record.LocalConfigFileHash ?? "(none)",
                        currentHash ?? "(none)");

                    ApplyRawFields(record, rawFields);
                    record.LocalConfigFileHash = currentHash;
                    ResolveFields(record, existing);
                    record.UpdatedAt = DateTime.UtcNow;

                    // Re-resolve all descendants so inherited values propagate
                    ResolveDescendants(dirPath, existing);
                }
                // Unchanged hash → resolved fields are still valid, skip
            }
            else
            {
                var parent = GetParentRecord(dirPath, existing);
                var newRecord = new DirectoryConfig
                {
                    Id = Guid.NewGuid(),
                    DirectoryPath = dirPath,
                    ParentId = parent?.Id,
                    LocalConfigFileHash = currentHash,
                    UpdatedAt = DateTime.UtcNow
                };

                ApplyRawFields(newRecord, rawFields);
                ResolveFields(newRecord, existing);

                db.DirectoryConfigs.Add(newRecord);
                existing[dirPath] = newRecord;
            }
        }

        await db.SaveChangesAsync();

        return await db.DirectoryConfigs.ToDictionaryAsync(d => d.DirectoryPath);
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

        // Build ordered dictionary of fields that are explicitly set (non-null)
        var data = new Dictionary<string, object>();
        if (req.Author != null)      data["author"] = req.Author;
        if (req.Collection != null)  data["collection"] = req.Collection;
        if (req.Subcollection != null) data["subcollection"] = req.Subcollection;
        if (req.Category != null)    data["category"] = req.Category;
        if (req.Type != null)        data["type"] = req.Type;
        if (req.Supported.HasValue)  data["supported"] = req.Supported.Value;

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
    /// The record's own Raw values take priority; the first non-null value found wins.
    /// </summary>
    private static void ResolveFields(DirectoryConfig record, Dictionary<string, DirectoryConfig> allRecords)
    {
        string? author = record.RawAuthor;
        string? collection = record.RawCollection;
        string? subcollection = record.RawSubcollection;
        string? category = record.RawCategory;
        string? type = record.RawType;
        bool? supported = record.RawSupported;

        var current = GetParentRecord(record.DirectoryPath, allRecords);
        while (current != null && (author == null || collection == null || subcollection == null || category == null || type == null || supported == null))
        {
            author ??= current.RawAuthor;
            collection ??= current.RawCollection;
            subcollection ??= current.RawSubcollection;
            category ??= current.RawCategory;
            type ??= current.RawType;
            supported ??= current.RawSupported;
            current = GetParentRecord(current.DirectoryPath, allRecords);
        }

        record.Author = author;
        record.Collection = collection;
        record.Subcollection = subcollection;
        record.Category = category;
        record.Type = type;
        record.Supported = supported;
    }

    private static void ApplyRawFields(DirectoryConfig record, RawConfigFields? fields)
    {
        record.RawAuthor = fields?.Author;
        record.RawCollection = fields?.Collection;
        record.RawSubcollection = fields?.Subcollection;
        record.RawCategory = fields?.Category;
        record.RawType = fields?.Type;
        record.RawSupported = fields?.Supported;
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
                Author: TryGetString(parsed, "author"),
                Collection: TryGetString(parsed, "collection"),
                Subcollection: TryGetString(parsed, "subcollection"),
                Category: ValidateEnum(TryGetString(parsed, "category"), ["Bust", "Miniature", "Uncategorized"]),
                Type: ValidateEnum(TryGetString(parsed, "type"), ["Whole", "Part"]),
                Supported: TryGetBool(parsed, "supported")
            );
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to parse config file '{FilePath}'; treating as empty", filePath);
            return null;
        }
    }

    private static string? TryGetString(Dictionary<string, object> data, string propertyName)
    {
        // Case-insensitive property search
        foreach (var kvp in data)
        {
            if (string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase))
            {
                var value = kvp.Value?.ToString();
                return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
            }
        }
        return null;
    }

    private static string? ValidateEnum(string? value, string[] allowed)
    {
        if (value == null) return null;
        return Array.Find(allowed, a => string.Equals(a, value, StringComparison.OrdinalIgnoreCase));
    }

    private static bool? TryGetBool(Dictionary<string, object> data, string propertyName)
    {
        foreach (var kvp in data)
        {
            if (!string.Equals(kvp.Key, propertyName, StringComparison.OrdinalIgnoreCase)) continue;
            if (kvp.Value is bool b) return b;
            if (kvp.Value is string s && bool.TryParse(s, out var parsed)) return parsed;
        }
        return null;
    }

    private sealed record RawConfigFields(string? Author, string? Collection, string? Subcollection, string? Category, string? Type, bool? Supported);
}
