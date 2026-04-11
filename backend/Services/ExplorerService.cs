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
        new(StringComparer.OrdinalIgnoreCase) { "stl", "obj", "lys", "lyt", "ctb" };

    private static readonly HashSet<string> ImageExtensions =
        new(StringComparer.OrdinalIgnoreCase) { "png", "jpg", "jpeg", "gif", "webp" };

    private static readonly HashSet<string> TextExtensions =
        new(StringComparer.OrdinalIgnoreCase) { "txt", "md" };

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

        // ---- Previewable non-model files ----
        var previewableFiles = Directory.GetFiles(fullPath)
            .Select(f => new FileInfo(f))
            .Where(f =>
            {
                var ext = f.Extension.TrimStart('.').ToLowerInvariant();
                return ImageExtensions.Contains(ext) || TextExtensions.Contains(ext);
            })
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
                ? new ConfigFieldsDto
                {
                    Creator = dc.Creator,
                    Collection = dc.Collection,
                    Subcollection = dc.Subcollection,
                    Category = dc.Category,
                    Type = dc.Type,
                    Material = dc.Material,
                    Supported = dc.Supported,
                    RaftHeightMm = dc.RaftHeightMm,
                    ModelName = dc.ModelName,
                    PartName = dc.PartName,
                }
                : new ConfigFieldsDto
                {
                    Creator = currentDirConfig?.Creator,
                    Collection = currentDirConfig?.Collection,
                    Subcollection = currentDirConfig?.Subcollection,
                    Category = currentDirConfig?.Category,
                    Type = currentDirConfig?.Type,
                    Material = currentDirConfig?.Material,
                    Supported = currentDirConfig?.Supported,
                    RaftHeightMm = currentDirConfig?.RaftHeightMm,
                    ModelName = currentDirConfig?.ModelName,
                    PartName = currentDirConfig?.PartName,
                };

            var localValues = dc != null
                ? new ConfigFieldsDto
                {
                    Creator = dc.RawCreator,
                    Collection = dc.RawCollection,
                    Subcollection = dc.RawSubcollection,
                    Category = dc.RawCategory,
                    Type = dc.RawType,
                    Material = dc.RawMaterial,
                    Supported = dc.RawSupported,
                    RaftHeightMm = dc.RawRaftHeightMm,
                    ModelName = dc.RawModelName,
                    PartName = dc.RawPartName,
                }
                : new ConfigFieldsDto();

            var ruleConfigs = BuildRuleConfigs(dc?.ResolvedRulesYaml);
            if (dc == null && currentDirConfig != null)
                ruleConfigs = BuildRuleConfigs(currentDirConfig.ResolvedRulesYaml);

            var localRuleFields = BuildLocalRuleFields(dc?.RawRulesYaml);

            // Evaluate rules against the folder's own path so the chip shows the computed value
            var folderRulesYaml = dc != null ? dc.ResolvedRulesYaml : currentDirConfig?.ResolvedRulesYaml;
            var folderRules = RuleRegistry.DeserializeRules(folderRulesYaml);
            if (folderRules.Count > 0)
                resolved = ApplyRulesToPath(resolved, folderRules, childFullPath);

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
                PreviewUrl: cm?.PreviewImagePath != null ? $"{previewBase}/{cm.Id}/preview?v={cm.PreviewGenerationVersion ?? 0}" : null,
                ResolvedMetadata: resolvedMeta,
                RuleConfigs: ruleConfigs);
        }).ToList();

        var parentPath = relativePath == ""
            ? null
            : (relativePath.Contains('/') ? relativePath[..relativePath.LastIndexOf('/')] : "");

        var files = previewableFiles.Select(fi =>
        {
            var relPath = relativePath == "" ? fi.Name : $"{relativePath}/{fi.Name}";
            var ext = fi.Extension.TrimStart('.').ToLowerInvariant();
            return new ExplorerFileItemDto(fi.Name, relPath, ext, fi.Length);
        }).ToList();

        return new ExplorerResponseDto(relativePath, parentPath, folders, models, files);
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

        var plain = new ModelMetadataHelper.ComputedMetadata
        {
            Creator = dc?.Creator,
            Collection = dc?.Collection,
            Subcollection = dc?.Subcollection,
            Category = dc?.Category,
            Type = dc?.Type,
            Material = dc?.Material,
            Supported = dc?.Supported,
            RaftHeightMm = dc?.RaftHeightMm,
            ModelName = dc?.ModelName,
            PartName = dc?.PartName,
        };

        var (resolved, ruleConfigs) = ModelMetadataHelper.ResolveForPath(
            plain, resolvedRules, fullFilePath, collectRuleConfigs: true);

        var dto = new ConfigFieldsDto
        {
            Creator = resolved.Creator,
            Collection = resolved.Collection,
            Subcollection = resolved.Subcollection,
            Category = resolved.Category,
            Type = resolved.Type,
            Material = resolved.Material,
            Supported = resolved.Supported,
            RaftHeightMm = resolved.RaftHeightMm,
            ModelName = resolved.ModelName,
            PartName = resolved.PartName,
        };

        // Return null metadata if all fields are null
        if (dto.Creator == null && dto.Collection == null && dto.Subcollection == null &&
            dto.Category == null && dto.Type == null && dto.Material == null &&
            dto.Supported == null && dto.RaftHeightMm == null && dto.ModelName == null &&
            dto.PartName == null)
            return (null, null);

        return (dto, ruleConfigs);
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
    /// Evaluates <paramref name="rules"/> against <paramref name="fullPath"/> and returns a new
    /// ConfigFieldsDto with rule-computed values merged in (rule values take precedence over plain).
    /// </summary>
    private static ConfigFieldsDto ApplyRulesToPath(
        ConfigFieldsDto plain,
        Dictionary<string, JsonElement> rules,
        string fullPath)
    {
        var plainMeta = new ModelMetadataHelper.ComputedMetadata
        {
            Creator = plain.Creator,
            Collection = plain.Collection,
            Subcollection = plain.Subcollection,
            Category = plain.Category,
            Type = plain.Type,
            Material = plain.Material,
            Supported = plain.Supported,
            RaftHeightMm = plain.RaftHeightMm,
            ModelName = plain.ModelName,
            PartName = plain.PartName,
        };

        var (resolved, _) = ModelMetadataHelper.ResolveForPath(plainMeta, rules, fullPath);

        return new ConfigFieldsDto
        {
            Creator = resolved.Creator,
            Collection = resolved.Collection,
            Subcollection = resolved.Subcollection,
            Category = resolved.Category,
            Type = resolved.Type,
            Material = resolved.Material,
            Supported = resolved.Supported,
            RaftHeightMm = resolved.RaftHeightMm,
            ModelName = resolved.ModelName,
            PartName = resolved.PartName,
        };
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
                JsonValueKind.True => "true",
                JsonValueKind.False => "false",
                JsonValueKind.Number => prop.Value.GetRawText(),
                _ => prop.Value.GetRawText()
            };
            sb.AppendLine($"  {prop.Name}: {val}");
        }
        return sb.ToString().TrimEnd();
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
