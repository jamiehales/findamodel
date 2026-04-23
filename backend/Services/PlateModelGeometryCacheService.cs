using System.Collections.Concurrent;
using findamodel.Models;

namespace findamodel.Services;

public sealed class PlateModelGeometryCacheService(
    ModelLoaderService loaderService,
    ModelRepairService repairService)
{
    private readonly ConcurrentDictionary<Guid, CacheEntry> cacheByModelId = new();
    private readonly ConcurrentDictionary<Guid, SemaphoreSlim> loadGatesByModelId = new();

    public async Task<LoadedGeometry> GetOrLoadAsync(
        Guid modelId,
        string expectedChecksum,
        string fullPath,
        string fileType,
        ModelRepairOptions repairOptions,
        CancellationToken cancellationToken)
    {
        var repairOptionsHash = repairOptions.ComputeDeterministicHash();

        if (cacheByModelId.TryGetValue(modelId, out var existing)
            && string.Equals(existing.Checksum, expectedChecksum, StringComparison.Ordinal)
            && string.Equals(existing.FullPath, fullPath, StringComparison.Ordinal)
            && string.Equals(existing.FileType, fileType, StringComparison.OrdinalIgnoreCase)
            && string.Equals(existing.RepairVersion, ModelRepairService.CurrentRepairVersion, StringComparison.Ordinal)
            && string.Equals(existing.RepairOptionsHash, repairOptionsHash, StringComparison.Ordinal))
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
                && string.Equals(existing.FileType, fileType, StringComparison.OrdinalIgnoreCase)
                && string.Equals(existing.RepairVersion, ModelRepairService.CurrentRepairVersion, StringComparison.Ordinal)
                && string.Equals(existing.RepairOptionsHash, repairOptionsHash, StringComparison.Ordinal))
            {
                return existing.Geometry;
            }

            var geometry = await loaderService.LoadModelAsync(fullPath, fileType);
            if (geometry is null)
                throw new InvalidOperationException($"Failed to parse geometry for: {Path.GetFileName(fullPath)}");

            var repaired = await repairService.RepairForSlicingAsync(geometry, repairOptions, cancellationToken);

            cacheByModelId[modelId] = new CacheEntry(
                expectedChecksum,
                fullPath,
                fileType,
                repaired.Geometry,
                ModelRepairService.CurrentRepairVersion,
                repairOptionsHash);

            return repaired.Geometry;
        }
        finally
        {
            gate.Release();
        }
    }

    private sealed record CacheEntry(
        string Checksum,
        string FullPath,
        string FileType,
        LoadedGeometry Geometry,
        string RepairVersion,
        string RepairOptionsHash);
}
