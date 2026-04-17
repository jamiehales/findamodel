using System.IO.Compression;
using System.Text.Json;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace findamodel.Services;

public sealed class PlateSliceRasterService(IEnumerable<IPlateSliceBitmapGenerator> generators)
{
    public const float DefaultLayerHeightMm = 0.05f;
    public const PngSliceExportMethod DefaultZipDownloadMethod = PngSliceExportMethod.MeshIntersection;

    private const int DefaultLayerBatchSize = 8;
    private const int MaxPendingEncodedLayers = 12;

    private readonly Dictionary<PngSliceExportMethod, IPlateSliceBitmapGenerator> generatorsByMethod =
        generators.ToDictionary(g => g.Method);

    public SliceBitmap RenderLayerBitmap(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int resolutionX,
        int resolutionY,
        PngSliceExportMethod method,
        float layerHeightMm = DefaultLayerHeightMm)
        => RenderLayerBitmap([triangles], sliceHeightMm, bedWidthMm, bedDepthMm, resolutionX, resolutionY, method, layerHeightMm);

    public SliceBitmap RenderLayerBitmap(
        IReadOnlyList<IReadOnlyList<Triangle3D>> triangleGroups,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int resolutionX,
        int resolutionY,
        PngSliceExportMethod method,
        float layerHeightMm = DefaultLayerHeightMm)
    {
        ValidateInputs(bedWidthMm, bedDepthMm, resolutionX, resolutionY, layerHeightMm);

        var generator = GetGenerator(method);
        var nonEmptyGroups = triangleGroups.Where(group => group.Count > 0).ToArray();
        if (nonEmptyGroups.Length == 0)
            return new SliceBitmap(resolutionX, resolutionY);

        if (nonEmptyGroups.Length == 1)
            return generator.RenderLayerBitmap(
                nonEmptyGroups[0],
                sliceHeightMm,
                bedWidthMm,
                bedDepthMm,
                resolutionX,
                resolutionY,
                layerHeightMm);

        var composed = new SliceBitmap(resolutionX, resolutionY);
        foreach (var group in nonEmptyGroups)
        {
            var bitmap = generator.RenderLayerBitmap(
                group,
                sliceHeightMm,
                bedWidthMm,
                bedDepthMm,
                resolutionX,
                resolutionY,
                layerHeightMm);
            OrInto(composed, bitmap);
        }

        return composed;
    }

    public byte[] GenerateSliceArchive(
        IReadOnlyList<Triangle3D> triangles,
        float bedWidthMm,
        float bedDepthMm,
        int resolutionX,
        int resolutionY,
        PngSliceExportMethod? method = null,
        float layerHeightMm = DefaultLayerHeightMm,
        IPlateGenerationProgressReporter? progressReporter = null,
        CancellationToken cancellationToken = default)
        => GenerateSliceArchive([triangles], bedWidthMm, bedDepthMm, resolutionX, resolutionY, method, layerHeightMm, progressReporter, cancellationToken);

    public byte[] GenerateSliceArchive(
        IReadOnlyList<IReadOnlyList<Triangle3D>> triangleGroups,
        float bedWidthMm,
        float bedDepthMm,
        int resolutionX,
        int resolutionY,
        PngSliceExportMethod? method = null,
        float layerHeightMm = DefaultLayerHeightMm,
        IPlateGenerationProgressReporter? progressReporter = null,
        CancellationToken cancellationToken = default)
    {
        ValidateInputs(bedWidthMm, bedDepthMm, resolutionX, resolutionY, layerHeightMm);

        var selectedMethod = method ?? DefaultZipDownloadMethod;
        var generator = GetGenerator(selectedMethod);
        var nonEmptyGroups = triangleGroups.Where(group => group.Count > 0).ToArray();
        var maxY = nonEmptyGroups.Length == 0
            ? 0f
            : nonEmptyGroups.Max(group => group.Max(t => MathF.Max(t.V0.Y, MathF.Max(t.V1.Y, t.V2.Y))));

        var layerCount = Math.Max(1, (int)Math.Ceiling(Math.Max(0f, maxY) / layerHeightMm));
        var trianglesByGroup = nonEmptyGroups
            .Select(group => BuildLayerBuckets(group, layerHeightMm, layerCount))
            .ToArray();

        progressReporter?.StartStage(layerCount, "Preparing slices");

        using var ms = new MemoryStream();
        using (var archive = new ZipArchive(ms, ZipArchiveMode.Create, leaveOpen: true))
        {
            WriteManifest(archive, bedWidthMm, bedDepthMm, resolutionX, resolutionY, layerHeightMm, layerCount, selectedMethod);

            var nextLayerToWrite = 0;
            var pendingEncodedLayers = new Dictionary<int, Task<byte[]>>();
            var batchSize = DefaultLayerBatchSize;

            for (var batchStart = 0; batchStart < layerCount; batchStart += batchSize)
            {
                cancellationToken.ThrowIfCancellationRequested();

                var batchLength = Math.Min(batchSize, layerCount - batchStart);
                var batchSliceHeights = new float[batchLength];
                for (var offset = 0; offset < batchLength; offset++)
                    batchSliceHeights[offset] = ((batchStart + offset) * layerHeightMm) + (layerHeightMm * 0.5f);

                progressReporter?.MarkCurrentEntry($"Slice layers {batchStart + 1}-{batchStart + batchLength} of {layerCount}");

                var renderedBatch = RenderCompositeBatch(
                    trianglesByGroup,
                    batchStart,
                    batchLength,
                    batchSliceHeights,
                    bedWidthMm,
                    bedDepthMm,
                    resolutionX,
                    resolutionY,
                    layerHeightMm,
                    generator,
                    cancellationToken);

                for (var offset = 0; offset < batchLength; offset++)
                {
                    var layerIndex = batchStart + offset;
                    var bitmap = renderedBatch[offset];
                    pendingEncodedLayers[layerIndex] = Task.Run(() => EncodePng(bitmap), cancellationToken);
                }

                FlushPendingEntries(archive, pendingEncodedLayers, ref nextLayerToWrite, forceWait: pendingEncodedLayers.Count >= MaxPendingEncodedLayers, progressReporter);
            }

            FlushPendingEntries(archive, pendingEncodedLayers, ref nextLayerToWrite, forceWait: true, progressReporter);
        }

        return ms.ToArray();
    }

    private static IReadOnlyList<Triangle3D>[] BuildLayerBuckets(
        IReadOnlyList<Triangle3D> triangles,
        float layerHeightMm,
        int layerCount)
    {
        var counts = new int[layerCount];

        foreach (var triangle in triangles)
        {
            var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
            var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            if (maxY < 0f)
                continue;

            var startLayer = Math.Clamp((int)MathF.Floor(MathF.Max(0f, minY) / layerHeightMm), 0, layerCount - 1);
            var endLayer = Math.Clamp((int)MathF.Ceiling(MathF.Max(0f, maxY) / layerHeightMm) - 1, 0, layerCount - 1);
            if (endLayer < startLayer)
                continue;

            for (var layerIndex = startLayer; layerIndex <= endLayer; layerIndex++)
                counts[layerIndex]++;
        }

        var byLayer = new List<Triangle3D>?[layerCount];
        for (var i = 0; i < layerCount; i++)
        {
            if (counts[i] > 0)
                byLayer[i] = new List<Triangle3D>(counts[i]);
        }

        foreach (var triangle in triangles)
        {
            var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
            var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            if (maxY < 0f)
                continue;

            var startLayer = Math.Clamp((int)MathF.Floor(MathF.Max(0f, minY) / layerHeightMm), 0, layerCount - 1);
            var endLayer = Math.Clamp((int)MathF.Ceiling(MathF.Max(0f, maxY) / layerHeightMm) - 1, 0, layerCount - 1);
            if (endLayer < startLayer)
                continue;

            for (var layerIndex = startLayer; layerIndex <= endLayer; layerIndex++)
                byLayer[layerIndex]!.Add(triangle);
        }

        return byLayer.Select(layer => (IReadOnlyList<Triangle3D>)(layer ?? [])).ToArray();
    }

    private static SliceBitmap[] RenderCompositeBatch(
        IReadOnlyList<Triangle3D>[][] trianglesByGroup,
        int batchStart,
        int batchLength,
        IReadOnlyList<float> batchSliceHeights,
        float bedWidthMm,
        float bedDepthMm,
        int resolutionX,
        int resolutionY,
        float layerHeightMm,
        IPlateSliceBitmapGenerator generator,
        CancellationToken cancellationToken)
    {
        var composed = Enumerable.Range(0, batchLength)
            .Select(_ => new SliceBitmap(resolutionX, resolutionY))
            .ToArray();

        foreach (var groupLayers in trianglesByGroup)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var batchTriangles = new IReadOnlyList<Triangle3D>[batchLength];
            var hasAnyTriangles = false;
            for (var offset = 0; offset < batchLength; offset++)
            {
                batchTriangles[offset] = groupLayers[batchStart + offset];
                hasAnyTriangles |= batchTriangles[offset].Count > 0;
            }

            if (!hasAnyTriangles)
                continue;

            var rendered = generator is IBatchPlateSliceBitmapGenerator batchGenerator && batchLength > 1
                ? batchGenerator.RenderLayerBitmaps(
                    batchTriangles,
                    batchSliceHeights,
                    bedWidthMm,
                    bedDepthMm,
                    resolutionX,
                    resolutionY,
                    layerHeightMm)
                : batchTriangles
                    .AsParallel()
                    .AsOrdered()
                    .Select((layerTriangles, offset) => generator.RenderLayerBitmap(
                        layerTriangles,
                        batchSliceHeights[offset],
                        bedWidthMm,
                        bedDepthMm,
                        resolutionX,
                        resolutionY,
                        layerHeightMm))
                    .ToArray();

            for (var offset = 0; offset < batchLength; offset++)
                OrInto(composed[offset], rendered[offset]);
        }

        return composed;
    }

    private static void OrInto(SliceBitmap target, SliceBitmap source)
    {
        for (var i = 0; i < target.Pixels.Length; i++)
        {
            if (source.Pixels[i] > 0)
                target.Pixels[i] = byte.MaxValue;
        }
    }

    private IPlateSliceBitmapGenerator GetGenerator(PngSliceExportMethod method)
    {
        if (generatorsByMethod.TryGetValue(method, out var generator))
            return generator;

        throw new InvalidOperationException($"No slice bitmap generator registered for {method}.");
    }

    private static void ValidateInputs(float bedWidthMm, float bedDepthMm, int resolutionX, int resolutionY, float layerHeightMm)
    {
        if (bedWidthMm <= 0 || bedDepthMm <= 0)
            throw new ArgumentException("Bed dimensions must be positive.");
        if (resolutionX <= 0 || resolutionY <= 0)
            throw new ArgumentException("Slice resolution must be positive.");
        if (layerHeightMm <= 0)
            throw new ArgumentException("Layer height must be positive.");
    }

    private static void WriteManifest(
        ZipArchive archive,
        float bedWidthMm,
        float bedDepthMm,
        int resolutionX,
        int resolutionY,
        float layerHeightMm,
        int layerCount,
        PngSliceExportMethod method)
    {
        var entry = archive.CreateEntry("manifest.json", CompressionLevel.Fastest);
        using var entryStream = entry.Open();
        JsonSerializer.Serialize(entryStream, new
        {
            format = "png-slice-stack",
            method = method.ToString(),
            layerHeightMm,
            layerCount,
            resolutionX,
            resolutionY,
            bedWidthMm,
            bedDepthMm,
            layerBatchSize = DefaultLayerBatchSize,
            generatedAtUtc = DateTime.UtcNow,
        }, new JsonSerializerOptions { WriteIndented = true });
    }

    private static void FlushPendingEntries(
        ZipArchive archive,
        Dictionary<int, Task<byte[]>> pendingEncodedLayers,
        ref int nextLayerToWrite,
        bool forceWait,
        IPlateGenerationProgressReporter? progressReporter)
    {
        while (pendingEncodedLayers.TryGetValue(nextLayerToWrite, out var pngTask))
        {
            if (!forceWait && !pngTask.IsCompleted)
                break;

            var entry = archive.CreateEntry($"slices/layer_{nextLayerToWrite:D5}.png", CompressionLevel.Fastest);
            using var entryStream = entry.Open();
            var pngBytes = pngTask.GetAwaiter().GetResult();
            entryStream.Write(pngBytes, 0, pngBytes.Length);
            pendingEncodedLayers.Remove(nextLayerToWrite);
            nextLayerToWrite++;
            progressReporter?.MarkEntryCompleted();
        }
    }

    private static byte[] EncodePng(SliceBitmap bitmap)
    {
        using var ms = new MemoryStream();
        WritePng(bitmap, ms);
        return ms.ToArray();
    }

    private static void WritePng(SliceBitmap bitmap, Stream stream)
    {
        using var image = Image.LoadPixelData<L8>(bitmap.Pixels, bitmap.Width, bitmap.Height);
        image.SaveAsPng(stream);
    }
}
