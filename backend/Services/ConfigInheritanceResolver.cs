using System.Text.Json;
using findamodel.Data.Entities;
using findamodel.Services.Rules;

namespace findamodel.Services;

/// <summary>
/// Resolves inherited metadata fields across the DirectoryConfig tree and
/// propagates changes to descendant records.
/// All methods are static and operate on in-memory dictionaries; callers
/// are responsible for saving changes to the database.
/// </summary>
internal static class ConfigInheritanceResolver
{
    internal static string? GetParentPath(string dirPath)
    {
        if (dirPath == "") return null;
        var lastSlash = dirPath.LastIndexOf('/');
        return lastSlash < 0 ? "" : dirPath[..lastSlash];
    }

    internal static DirectoryConfig? GetParentRecord(string dirPath, Dictionary<string, DirectoryConfig> existing)
    {
        var parentPath = GetParentPath(dirPath);
        if (parentPath == null) return null;
        return existing.TryGetValue(parentPath, out var parent) ? parent : null;
    }

    internal static string GetFullDirPath(string rootPath, string dirPath) =>
        dirPath == ""
            ? rootPath
            : Path.Combine(rootPath, dirPath.Replace('/', Path.DirectorySeparatorChar));

    internal static string BuildFullFilePath(string modelsRootPath, string directory, string fileName) =>
        string.IsNullOrEmpty(directory)
            ? Path.Combine(modelsRootPath, fileName)
            : Path.Combine(modelsRootPath, directory.Replace('/', Path.DirectorySeparatorChar), fileName);

    /// <summary>
    /// Expands a set of relative directory paths to include all ancestor paths.
    /// E.g. "Fantasy/Elite" → {"", "Fantasy", "Fantasy/Elite"}.
    /// Root ("") is always included.
    /// </summary>
    internal static HashSet<string> ExpandToAllAncestors(IEnumerable<string> paths)
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
    /// Expands a set of changed root directories to include all their descendants from allKnownDirs.
    /// </summary>
    internal static List<string> ExpandToDescendants(HashSet<string> changedRoots, IEnumerable<string> allKnownDirs)
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

    internal static void ApplyRawFields(DirectoryConfig record, RawConfigFields? fields)
    {
        record.RawCreator = fields?.Creator;
        record.RawCollection = fields?.Collection;
        record.RawSubcollection = fields?.Subcollection;
        record.RawCategory = fields?.Category;
        record.RawType = fields?.Type;
        record.RawMaterial = fields?.Material;
        record.RawSupported = fields?.Supported;
        record.RawRaftHeightMm = fields?.RaftHeightMm;
        record.RawModelName = fields?.ModelName;
        record.RawRulesYaml = fields?.RulesYaml;
    }

    /// <summary>
    /// Re-resolves all descendants of <paramref name="parentPath"/> using the provided
    /// in-memory dictionary. Updates resolved fields and UpdatedAt in place.
    /// Does NOT call SaveChanges — callers are responsible for saving.
    /// </summary>
    internal static void ResolveDescendants(string parentPath, Dictionary<string, DirectoryConfig> all)
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

    /// <summary>
    /// Resolves all metadata fields by walking up the ancestor chain.
    /// Uses Raw* fields only (never resolved fields) to prevent cascading errors.
    /// The record's own Raw values take priority; the first non-null value or rule found wins.
    /// A field resolved to a rule is stored in ResolvedRulesYaml; plain values use the existing columns.
    /// </summary>
    internal static void ResolveFields(DirectoryConfig record, Dictionary<string, DirectoryConfig> allRecords)
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

        if (record.RawRaftHeightMm.HasValue)
        {
            record.RaftHeightMm = record.RawRaftHeightMm;
        }
        else
        {
            var raftHeightFromParent = (float?)null;
            var parent = GetParentRecord(record.DirectoryPath, allRecords);
            while (parent != null)
            {
                if (parent.RawRaftHeightMm.HasValue)
                {
                    raftHeightFromParent = parent.RawRaftHeightMm;
                    break;
                }
                parent = GetParentRecord(parent.DirectoryPath, allRecords);
            }
            record.RaftHeightMm = raftHeightFromParent;
        }

        record.ResolvedRulesYaml = resolvedRules.Count > 0
            ? DirectoryConfigReader.SerializeRulesToYaml(resolvedRules)
            : null;
    }
}
