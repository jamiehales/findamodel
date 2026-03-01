using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Models;

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
                ? new ConfigFieldsDto(dc.Author, dc.Collection, dc.Subcollection,
                                      dc.Category, dc.Type, dc.Supported)
                : new ConfigFieldsDto(null, null, null, null, null, null);

            folders.Add(new FolderItemDto(name, childPath, subdirCount, modelCount, resolved));
        }

        // ---- Build model items ----
        var previewBase = "/api/models";
        var models = modelFileNames.Select(fi =>
        {
            var relPath = relativePath == "" ? fi.Name : $"{relativePath}/{fi.Name}";
            cachedModels.TryGetValue(fi.Name, out var cm);

            return new ExplorerModelItemDto(
                Id: cm?.Id.ToString(),
                FileName: fi.Name,
                RelativePath: relPath,
                FileType: fi.Extension.TrimStart('.').ToLower(),
                FileSize: cm?.FileSize ?? fi.Length,
                HasPreview: cm?.PreviewImagePath != null,
                PreviewUrl: cm?.PreviewImagePath != null ? $"{previewBase}/{cm.Id}/preview" : null);
        }).ToList();

        var parentPath = relativePath == ""
            ? null
            : (relativePath.Contains('/') ? relativePath[..relativePath.LastIndexOf('/')] : "");

        return new ExplorerResponseDto(relativePath, parentPath, folders, models);
    }

    // ---- Helpers ----

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
