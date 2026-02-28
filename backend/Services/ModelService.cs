using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class ModelService(
    IConfiguration config,
    ILogger<ModelService> logger,
    IDbContextFactory<ModelCacheContext> dbFactory,
    ModelPreviewService previewService)
{
    private static readonly string[] ModelExtensions = [".stl", ".obj"];

    public async Task<List<ModelDto>> GetModelsAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        // Project to avoid loading the PreviewImage BLOB into memory for list queries
        var rows = await db.Models
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName)
            .Select(m => new
            {
                m.Id,
                m.FileName,
                m.Directory,
                m.FileType,
                m.FileSize,
                HasPreview = m.PreviewImage != null
            })
            .ToListAsync();

        return rows.Select(m => new ModelDto
        {
            Id = m.Id,
            Name = Path.GetFileNameWithoutExtension(m.FileName),
            RelativePath = ComputeRelativePath(m.Directory, m.FileName),
            FileType = m.FileType,
            FileSize = m.FileSize,
            FileUrl = $"/api/models/{m.Id}/file",
            HasPreview = m.HasPreview,
            PreviewUrl = m.HasPreview ? $"/api/models/{m.Id}/preview" : null
        }).ToList();
    }

    public async Task<CachedModel?> GetModelAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        return await db.Models.FirstOrDefaultAsync(m => m.Id == id);
    }

    public async Task<byte[]?> GetPreviewImageAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        return await db.Models
            .Where(m => m.Id == id)
            .Select(m => m.PreviewImage)
            .FirstOrDefaultAsync();
    }

    public async Task ScanAndCacheAsync()
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

        await using var db = await dbFactory.CreateDbContextAsync();
        var existingChecksums = (await db.Models.Select(m => m.Checksum).ToListAsync()).ToHashSet();

        var files = Directory.EnumerateFiles(modelsPath, "*.*", SearchOption.AllDirectories)
            .Where(f => ModelExtensions.Contains(Path.GetExtension(f).ToLower()));

        var newCount = 0;
        foreach (var file in files)
        {
            var checksum = await ComputeChecksumAsync(file);
            if (existingChecksums.Contains(checksum)) continue;

            var relativePath = Path.GetRelativePath(modelsPath, file);
            var fileName = Path.GetFileName(file);
            var directory = (Path.GetDirectoryName(relativePath) ?? "").Replace('\\', '/');
            var info = new FileInfo(file);
            var fileType = Path.GetExtension(file).TrimStart('.').ToLower();

            logger.LogInformation("Discovereddddd {FileType} model: {FilePath}", fileType.ToUpper(), file);

            var preview = await previewService.GeneratePreviewAsync(file, fileType);

            var entity = new CachedModel
            {
                Id = Guid.NewGuid(),
                Checksum = checksum,
                FileName = fileName,
                Directory = directory,
                FileType = fileType,
                FileSize = info.Length,
                FileModifiedAt = info.LastWriteTimeUtc,
                CachedAt = DateTime.UtcNow,
                PreviewImage = preview,
                PreviewGeneratedAt = preview != null ? DateTime.UtcNow : null
            };

            db.Models.Add(entity);
            await db.SaveChangesAsync();
            existingChecksums.Add(checksum);
            newCount++;
        }

        logger.LogInformation("Scan complete: {New} new models added, {Total} total in cache.", newCount, await db.Models.CountAsync());
    }

    private static async Task<string> ComputeChecksumAsync(string filePath)
    {
        using var stream = File.OpenRead(filePath);
        var hash = await System.Security.Cryptography.SHA256.HashDataAsync(stream);
        return Convert.ToHexString(hash).ToLower();
    }

    private static string ComputeRelativePath(string directory, string fileName) =>
        string.IsNullOrEmpty(directory) ? fileName : $"{directory}/{fileName}";
}
