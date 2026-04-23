using System.Collections.Concurrent;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Formats.Png;
using findamodel.Models;

namespace findamodel.Services;

public sealed class PlateSlicePreviewService(
    PlateExportService plateExportService,
    PlateSliceRasterService sliceRasterService)
{
    private static readonly TimeSpan PreviewRetention = TimeSpan.FromMinutes(15);

    private readonly ConcurrentDictionary<Guid, PreviewSession> sessions = new();

    public async Task<PlateSlicePreviewSessionDto> CreateAsync(
        CreatePlateSlicePreviewRequest request,
        CancellationToken cancellationToken = default)
    {
        CleanupExpired();

        var previewData = await plateExportService.BuildSlicePreviewDataAsync(
            new GeneratePlateRequest(request.Placements, request.Format, request.PrinterConfigId),
            request.Method,
            cancellationToken);

        var now = DateTime.UtcNow;
        var session = new PreviewSession(
            Guid.NewGuid(),
            now,
            now.Add(PreviewRetention),
            previewData.TriangleGroups,
            previewData.BedWidthMm,
            previewData.BedDepthMm,
            previewData.ResolutionX,
            previewData.ResolutionY,
            previewData.LayerHeightMm,
            previewData.LayerCount,
            previewData.Method,
            previewData.Warning,
            previewData.SkippedModels);

        sessions[session.PreviewId] = session;
        return session.ToDto();
    }

    public PlateSlicePreviewSessionDto? GetSession(Guid previewId)
    {
        CleanupExpired();
        if (!sessions.TryGetValue(previewId, out var session))
            return null;

        if (session.ExpiresAtUtc <= DateTime.UtcNow)
        {
            sessions.TryRemove(previewId, out _);
            return null;
        }

        return session.ToDto();
    }

    public byte[]? RenderLayerPng(Guid previewId, int layerIndex)
    {
        CleanupExpired();
        if (!sessions.TryGetValue(previewId, out var session))
            return null;

        if (session.ExpiresAtUtc <= DateTime.UtcNow)
        {
            sessions.TryRemove(previewId, out _);
            return null;
        }

        if (layerIndex < 0 || layerIndex >= session.LayerCount)
            return null;

        var sliceHeightMm = (layerIndex * session.LayerHeightMm) + (session.LayerHeightMm * 0.5f);
        var bitmap = sliceRasterService.RenderLayerBitmap(
            session.TriangleGroups,
            sliceHeightMm,
            session.BedWidthMm,
            session.BedDepthMm,
            session.ResolutionX,
            session.ResolutionY,
            session.Method,
            session.LayerHeightMm);

        using var stream = new MemoryStream();
        using (var image = Image.LoadPixelData<L8>(bitmap.Pixels, bitmap.Width, bitmap.Height))
            image.Save(stream, new PngEncoder());

        return stream.ToArray();
    }

    private void CleanupExpired()
    {
        var now = DateTime.UtcNow;
        var expired = sessions
            .Where(pair => pair.Value.ExpiresAtUtc <= now)
            .Select(pair => pair.Key)
            .ToArray();

        foreach (var id in expired)
            sessions.TryRemove(id, out _);
    }

    private sealed record PreviewSession(
        Guid PreviewId,
        DateTime CreatedAtUtc,
        DateTime ExpiresAtUtc,
        IReadOnlyList<IReadOnlyList<Triangle3D>> TriangleGroups,
        float BedWidthMm,
        float BedDepthMm,
        int ResolutionX,
        int ResolutionY,
        float LayerHeightMm,
        int LayerCount,
        PngSliceExportMethod Method,
        string? Warning,
        IReadOnlyList<string> SkippedModels)
    {
        public PlateSlicePreviewSessionDto ToDto() => new(
            PreviewId,
            BedWidthMm,
            BedDepthMm,
            ResolutionX,
            ResolutionY,
            LayerHeightMm,
            LayerCount,
            Method.ToString(),
            CreatedAtUtc,
            ExpiresAtUtc,
            Warning,
            SkippedModels);
    }
}
