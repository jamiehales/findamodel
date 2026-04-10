using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services.Rules;

namespace findamodel.Services;

public class ExplorerService(
    IConfiguration configuration,
    IDbContextFactory<ModelCacheContext> dbFactory)
{
    // Extensible: add new supported file types here
    private static readonly HashSet<string> SupportedExtensions =
        new(StringComparer.OrdinalIgnoreCase) { "stl", "obj" };

    /// <summary>
    /// Lists the contents of <paramref name="relativePath"/> within the models root:
    /// subdirectories first (sorted), then model files (sorted). Each entry is augmented
    /// with any matching DB records (DirectoryConfig / CachedModel).
    /// </summary>
    public async Task<ExplorerResponseDto> GetDirectoryContentsAsync(string relativePath)
    {
        var modelsRoot = configuration["Models:DirectoryPath"]
            ?? throw new InvalidOperationException("Models:DirectoryPath is not configured.");

        var fullPath = relativePath == ""
            ? modelsRoot
            : Path.Combine(modelsRoot, relativePath.Replace('/', Path.DirectorySeparatorChar));

        if (!Directory.Exists(fullPath))
            throw new DirectoryNotFoundException($"Path not found: {relativePath}");

        // ---- Subdirectories ----
        var subdirNames = Directory.GetDirectories(fullPath)
            .Select(Path.GetFileName)
            .Where(n => n != null)
            .Select(n => n!)
            .OrderBy(n => n, StringComparer.OrdinalIgnoreCase)
            .ToList();

        var childPaths = subdirNames
            .Select(n => relativePath == "" ? n : $"{relativePath}/{n}")
            .ToList();

        // ---- Model files ----
        var modelFileNames = Directory.GetFiles(fullPath)
            .Select(f => new FileInfo(f))
            .Where(f => SupportedExtensions.Contains(f.Extension.TrimStart('.')))
            .OrderBy(f => f.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();

        // ---- DB lookups ----
        await using var db = await dbFactory.CreateDbContextAsync();

        var dirConfigs = await db.DirectoryConfigs
            .Where(d => childPaths.Contains(d.DirectoryPath))
            .ToDictionaryAsync(d => d.DirectoryPath);

        // Load the current directory's config for resolving model metadata and rules
        var currentDirConfig = await db.DirectoryConfigs
            .FirstOrDefaultAsync(d => d.DirectoryPath == relativePath);

        var cachedModels = await db.Models
            .Where(m => m.Directory == relativePath &&
                        modelFileNames.Select(f => f.Name).Contains(m.FileName))
            .ToDictionaryAsync(m => m.FileName);

        // ---- Build folder items ----
        var folders = new List<FolderItemDto>(subdirNames.Count);
        for (int i = 0; i < subdirNames.Count; i++)
        {
            var name = subdirNames[i];
            var childPath = childPaths[i];
            var childFullPath = Path.Combine(fullPath, name);

            var subdirCount = CountSubdirectories(childFullPath);
            var modelCount = CountModelFiles(childFullPath);

            dirConfigs.TryGetValue(childPath, out var dc);
            var resolved = dc != null
                ? new ConfigFieldsDto(dc.Creator, dc.Collection, dc.Subcollection,
                                      dc.Category, dc.Type, dc.Supported, dc.ModelName)
                : new ConfigFieldsDto(
                    currentDirConfig?.Creator,
                    currentDirConfig?.Collection,
                    currentDirConfig?.Subcollection,
                    currentDirConfig?.Category,
                    currentDirConfig?.Type,
                    currentDirConfig?.Supported,
                    currentDirConfig?.ModelName);

            var localValues = dc != null
                ? new ConfigFieldsDto(dc.RawCreator, dc.RawCollection, dc.RawSubcollection,
                                      dc.RawCategory, dc.RawType, dc.RawSupported, dc.RawModelName)
                : new ConfigFieldsDto(null, null, null, null, null, null);

            var ruleConfigs = BuildRuleConfigs(dc?.ResolvedRulesYaml);
            if (dc == null && currentDirConfig != null)
                ruleConfigs = BuildRuleConfigs(currentDirConfig.ResolvedRulesYaml);

            var localRuleFields = BuildLocalRuleFields(dc?.RawRulesYaml);

            folders.Add(new FolderItemDto(name, childPath, subdirCount, modelCount, resolved, ruleConfigs, localValues, localRuleFields));
        }

        // ---- Precompute resolved rules for current directory ----
        var resolvedRules = RuleRegistry.DeserializeRules(currentDirConfig?.ResolvedRulesYaml);

        // ---- Build model items ----
        var previewBase = "/api/models";
        var models = modelFileNames.Select(fi =>
        {
            var relPath = relativePath == "" ? fi.Name : $"{relativePath}/{fi.Name}";
            cachedModels.TryGetValue(fi.Name, out var cm);

            var (resolvedMeta, ruleConfigs) = BuildModelMetadata(currentDirConfig, resolvedRules, fi.FullName);

            return new ExplorerModelItemDto(
                Id: cm?.Id.ToString(),
                FileName: fi.Name,
                RelativePath: relPath,
                FileType: fi.Extension.TrimStart('.').ToLower(),
                FileSize: cm?.FileSize ?? fi.Length,
                HasPreview: cm?.PreviewImagePath != null,
                PreviewUrl: cm?.PreviewImagePath != null ? $"{previewBase}/{cm.Id}/preview" : null,
                ResolvedMetadata: resolvedMeta,
                RuleConfigs: ruleConfigs);
        }).ToList();

        var parentPath = relativePath == ""
            ? null
            : (relativePath.Contains('/') ? relativePath[..relativePath.LastIndexOf('/')] : "");

        return new ExplorerResponseDto(relativePath, parentPath, folders, models);
    }

    // ---- Helpers ----

    /// <summary>
    /// Computes the effective metadata for a model file by combining the directory's resolved plain
    /// values with any rule-evaluated values. Returns the metadata DTO and a map of rule-derived
    /// field names to their YAML snippets. Returns (null, null) if there is no config and no rules.
    /// </summary>
    private static (ConfigFieldsDto? Meta, Dictionary<string, string>? RuleConfigs) BuildModelMetadata(
        DirectoryConfig? dc,
        Dictionary<string, JsonElement> resolvedRules,
        string fullFilePath)
    {
        if (dc == null && resolvedRules.Count == 0) return (null, null);

        string? creator = dc?.Creator;
        string? collection = dc?.Collection;
        string? subcollection = dc?.Subcollection;
        string? category = dc?.Category;
        string? type = dc?.Type;
        bool? supported = dc?.Supported;
        string? modelName = dc?.ModelName;

        Dictionary<string, string>? ruleConfigs = null;

        if (resolvedRules.Count > 0)
        {
            // Build available (non-rule) fields to pass to parsers
            var available = new Dictionary<string, string?>(StringComparer.OrdinalIgnoreCase)
            {
                ["creator"] = creator,
                ["collection"] = collection,
                ["subcollection"] = subcollection,
                ["category"] = category,
                ["type"] = type,
                ["model_name"] = modelName,
            };
            foreach (var field in resolvedRules.Keys) available.Remove(field);

            var fieldTypes = new Dictionary<string, RuleFieldType>(StringComparer.OrdinalIgnoreCase)
            {
                ["supported"] = RuleFieldType.Bool,
                ["category"]  = RuleFieldType.Enum,
                ["type"]      = RuleFieldType.Enum,
            };

            ruleConfigs = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (var (field, ruleEl) in resolvedRules)
            {
                var ft = fieldTypes.TryGetValue(field, out var t) ? t : RuleFieldType.String;
                var value = RuleRegistry.Evaluate(field, fullFilePath, available, ruleEl, ft);
                if (value == null) continue;

                ruleConfigs[field.ToLowerInvariant()] = RuleConfigToYamlSnippet(field, ruleEl);
                switch (field.ToLowerInvariant())
                {
                    case "creator":     creator     = value; break;
                    case "collection":  collection  = value; break;
                    case "subcollection": subcollection = value; break;
                    case "category":    category    = ValidateEnumValue(value, ["Bust", "Miniature", "Uncategorized"]); break;
                    case "type":        type        = ValidateEnumValue(value, ["Whole", "Part"]); break;
                    case "model_name":  modelName   = value; break;
                    case "supported":   supported   = value.Equals("true", StringComparison.OrdinalIgnoreCase); break;
                }
            }

            if (ruleConfigs.Count == 0) ruleConfigs = null;
        }

        // Return null metadata if all fields are null
        if (creator == null && collection == null && subcollection == null
            && category == null && type == null && supported == null && modelName == null)
            return (null, null);

        return (new ConfigFieldsDto(creator, collection, subcollection, category, type, supported, modelName), ruleConfigs);
    }

    /// <summary>
    /// Builds a dictionary mapping each rule field to its YAML snippet for display in the UI.
    /// Returns null when there are no resolved rules.
    /// </summary>
    private static Dictionary<string, string>? BuildRuleConfigs(string? resolvedRulesYaml)
    {
        var rules = RuleRegistry.DeserializeRules(resolvedRulesYaml);
        if (rules.Count == 0) return null;

        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (field, ruleEl) in rules)
            result[field.ToLowerInvariant()] = RuleConfigToYamlSnippet(field, ruleEl);
        return result;
    }

    /// <summary>
    /// Returns local rule field names defined directly in this directory's YAML.
    /// </summary>
    private static HashSet<string>? BuildLocalRuleFields(string? rawRulesYaml)
    {
        var rules = RuleRegistry.DeserializeRules(rawRulesYaml);
        if (rules.Count == 0) return null;

        return rules.Keys
            .Select(k => k.ToLowerInvariant())
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Converts a rule's JsonElement config to a compact YAML snippet for display.
    /// E.g. {"rule":"filename","include_extension":false} →
    ///   collection:
    ///     rule: filename
    ///     include_extension: false
    /// </summary>
    private static string RuleConfigToYamlSnippet(string fieldName, JsonElement ruleEl)
    {
        var sb = new System.Text.StringBuilder();
        sb.AppendLine($"{fieldName}:");
        foreach (var prop in ruleEl.EnumerateObject())
        {
            var val = prop.Value.ValueKind switch
            {
                JsonValueKind.String => prop.Value.GetString() ?? "",
                JsonValueKind.True   => "true",
                JsonValueKind.False  => "false",
                JsonValueKind.Number => prop.Value.GetRawText(),
                _                   => prop.Value.GetRawText()
            };
            sb.AppendLine($"  {prop.Name}: {val}");
        }
        return sb.ToString().TrimEnd();
    }

    private static string? ValidateEnumValue(string? value, string[] allowed)
    {
        if (value == null) return null;
        return Array.Find(allowed, a => string.Equals(a, value, StringComparison.OrdinalIgnoreCase));
    }

    private static int CountSubdirectories(string fullPath)
    {
        try { return Directory.GetDirectories(fullPath).Length; }
        catch { return 0; }
    }

    private static int CountModelFiles(string fullPath)
    {
        try
        {
            return Directory.GetFiles(fullPath)
                .Count(f => SupportedExtensions.Contains(
                    Path.GetExtension(f).TrimStart('.').ToLower()));
        }
        catch { return 0; }
    }
}
