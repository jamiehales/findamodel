using System.Collections.Concurrent;

namespace findamodel.Services;

public sealed class PlateModelGeometryCacheService(ModelLoaderService loaderService)
{
    private readonly ConcurrentDictionary<Guid, CacheEntry> cacheByModelId = new();
    private readonly ConcurrentDictionary<Guid, SemaphoreSlim> loadGatesByModelId = new();

    public async Task<LoadedGeometry> GetOrLoadAsync(
        Guid modelId,
        string expectedChecksum,
        string fullPath,
        string fileType,
        CancellationToken cancellationToken)
    {
        if (cacheByModelId.TryGetValue(modelId, out var existing)
            && string.Equals(existing.Checksum, expectedChecksum, StringComparison.Ordinal)
            && string.Equals(existing.FullPath, fullPath, StringComparison.Ordinal)
            && string.Equals(existing.FileType, fileType, StringComparison.OrdinalIgnoreCase))
        {
            return existing.Geometry;
        }

        var gate = loadGatesByModelId.GetOrAdd(modelId, _ => new SemaphoreSlim(1, 1));
        await gate.WaitAsync(cancellationToken);
        try
        {
            if (cacheByModelId.TryGetValue(modelId, out existing)
                && string.Equals(existing.Checksum, expectedChecksum, StringComparison.Ordinal)
                && string.Equals(existing.FullPath, fullPath, StringComparison.Ordinal)
                && string.Equals(existing.FileType, fileType, StringComparison.OrdinalIgnoreCase))
            {
                return existing.Geometry;
            }

            var geometry = await loaderService.LoadModelAsync(fullPath, fileType);
            if (geometry is null)
                throw new InvalidOperationException($"Failed to parse geometry for: {Path.GetFileName(fullPath)}");

            cacheByModelId[modelId] = new CacheEntry(expectedChecksum, fullPath, fileType, geometry);
            return geometry;
        }
        finally
        {
            gate.Release();
        }
    }

    private sealed record CacheEntry(string Checksum, string FullPath, string FileType, LoadedGeometry Geometry);
}
