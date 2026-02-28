using System.Security.Cryptography;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;

namespace findamodel.Services;

public class MetadataConfigService(
    ILogger<MetadataConfigService> logger,
    IDbContextFactory<ModelCacheContext> dbFactory)
{
    private const string ConfigFileName = "findamodel.json";

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
            var fullPath = dirPath == ""
                ? modelsRootPath
                : Path.Combine(modelsRootPath, dirPath.Replace('/', Path.DirectorySeparatorChar));

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

                    await OnConfigFileChangedAsync(dirPath, record, rawFields, db);

                    ApplyRawFields(record, rawFields);
                    record.LocalConfigFileHash = currentHash;
                    ResolveFields(record, existing);
                    record.UpdatedAt = DateTime.UtcNow;
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
    // Stub: invoked when a previously-seen config file has a new hash
    // -------------------------------------------------------------------------

    /// <summary>
    /// Called when a config file's hash changes during a scan.
    /// Extend to re-resolve descendant DirectoryConfig records, invalidate caches, etc.
    /// </summary>
    private Task OnConfigFileChangedAsync(
        string directoryPath,
        DirectoryConfig existingRecord,
        RawConfigFields? newFields,
        ModelCacheContext db)
    {
        // TODO: re-resolve all descendant DirectoryConfig records that inherited values from this directory
        // TODO: notify any downstream consumers (audit log, SignalR hub, etc.)
        logger.LogInformation("OnConfigFileChangedAsync stub called for '{Dir}'", directoryPath);
        return Task.CompletedTask;
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

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
        if (dirPath == "") return null;
        var lastSlash = dirPath.LastIndexOf('/');
        var parentPath = lastSlash < 0 ? "" : dirPath[..lastSlash];
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
            await using var stream = File.OpenRead(filePath);
            var doc = await JsonDocument.ParseAsync(stream);
            var root = doc.RootElement;

            return new RawConfigFields(
                Author: TryGetString(root, "author"),
                Collection: TryGetString(root, "collection"),
                Subcollection: TryGetString(root, "subcollection"),
                Category: ValidateEnum(TryGetString(root, "category"), ["Bust", "Miniature", "Uncategorized"]),
                Type: ValidateEnum(TryGetString(root, "type"), ["Whole", "Part"]),
                Supported: TryGetBool(root, "supported")
            );
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to parse config file '{FilePath}'; treating as empty", filePath);
            return null;
        }
    }

    private static string? TryGetString(JsonElement element, string propertyName)
    {
        // Case-insensitive property search
        foreach (var prop in element.EnumerateObject())
        {
            if (string.Equals(prop.Name, propertyName, StringComparison.OrdinalIgnoreCase)
                && prop.Value.ValueKind == JsonValueKind.String)
            {
                var value = prop.Value.GetString();
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

    private static bool? TryGetBool(JsonElement element, string propertyName)
    {
        foreach (var prop in element.EnumerateObject())
        {
            if (!string.Equals(prop.Name, propertyName, StringComparison.OrdinalIgnoreCase)) continue;
            if (prop.Value.ValueKind == JsonValueKind.True) return true;
            if (prop.Value.ValueKind == JsonValueKind.False) return false;
        }
        return null;
    }

    private sealed record RawConfigFields(string? Author, string? Collection, string? Subcollection, string? Category, string? Type, bool? Supported);
}
