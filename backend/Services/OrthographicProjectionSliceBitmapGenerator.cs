using System.Runtime.InteropServices;
using Microsoft.Extensions.Logging;

namespace findamodel.Services;

public sealed class OrthographicProjectionSliceBitmapGenerator : IBatchPlateSliceBitmapGenerator
{
    private const bool EnableGpuSliceProjection = false;
    private const float Epsilon = 0.0001f;
    private const float RayOffsetMm = 0.0005f;
    private const float HitDedupEpsilon = 0.002f;
    private static readonly Vec3 RayDirection = new(1f, 0f, 0f);

    private readonly GlSliceProjectionContext? gpuContext;
    private readonly ILogger<OrthographicProjectionSliceBitmapGenerator>? logger;

    public OrthographicProjectionSliceBitmapGenerator()
    {
    }

    public OrthographicProjectionSliceBitmapGenerator(
        GlSliceProjectionContext gpuContext,
        ILoggerFactory loggerFactory)
    {
        this.gpuContext = gpuContext;
        logger = loggerFactory.CreateLogger<OrthographicProjectionSliceBitmapGenerator>();
    }

    public PngSliceExportMethod Method => PngSliceExportMethod.OrthographicProjection;

    public SliceBitmap RenderLayerBitmap(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm = PlateSliceRasterService.DefaultLayerHeightMm)
    {
        var gpuBitmap = TryRenderGpuBatch(
            [triangles],
            [sliceHeightMm],
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerThicknessMm);

        if (gpuBitmap is { Count: > 0 })
        {
            gpuBitmap[0].RemoveUnsupportedHorizontalPixels();
            return gpuBitmap[0];
        }

        return RenderLayerBitmapCpu(
            triangles,
            sliceHeightMm,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerThicknessMm);
    }

    public IReadOnlyList<SliceBitmap> RenderLayerBitmaps(
        IReadOnlyList<IReadOnlyList<Triangle3D>> trianglesByLayer,
        IReadOnlyList<float> sliceHeightsMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm = PlateSliceRasterService.DefaultLayerHeightMm)
    {
        if (trianglesByLayer.Count != sliceHeightsMm.Count)
            throw new ArgumentException("Layer triangle count and slice height count must match.");

        if (trianglesByLayer.Count == 0)
            return [];

        var gpuBitmaps = TryRenderGpuBatch(
            trianglesByLayer,
            sliceHeightsMm,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerThicknessMm);

        if (gpuBitmaps is not null)
        {
            foreach (var bitmap in gpuBitmaps)
                bitmap.RemoveUnsupportedHorizontalPixels();

            return gpuBitmaps;
        }

        var bitmaps = new SliceBitmap[trianglesByLayer.Count];

        if (trianglesByLayer.Count <= 2)
        {
            for (var index = 0; index < trianglesByLayer.Count; index++)
            {
                bitmaps[index] = RenderLayerBitmapCpu(
                    trianglesByLayer[index],
                    sliceHeightsMm[index],
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    layerThicknessMm);
            }
        }
        else
        {
            Parallel.For(0, trianglesByLayer.Count, new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount }, index =>
            {
                bitmaps[index] = RenderLayerBitmapCpuSerial(
                    trianglesByLayer[index],
                    sliceHeightsMm[index],
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    layerThicknessMm);
            });
        }

        return bitmaps;
    }

    private IReadOnlyList<SliceBitmap>? TryRenderGpuBatch(
        IReadOnlyList<IReadOnlyList<Triangle3D>> trianglesByLayer,
        IReadOnlyList<float> sliceHeightsMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm)
    {
        if (!EnableGpuSliceProjection || gpuContext is null || !gpuContext.IsAvailable)
            return null;

        try
        {
            List<Triangle3D> activeTriangles;
            if (trianglesByLayer.Count == 1)
            {
                activeTriangles = FilterActiveTrianglesForGpu(
                    trianglesByLayer[0],
                    sliceHeightsMm[0],
                    layerThicknessMm,
                    maxTriangleCount: 2_000_000);
            }
            else
            {
                var minSlice = sliceHeightsMm.Min() - (layerThicknessMm * 0.5f);
                var maxSlice = sliceHeightsMm.Max() + (layerThicknessMm * 0.5f);
                activeTriangles = BuildBatchTriangles(trianglesByLayer, minSlice, maxSlice, maxTriangleCount: 2_000_000);
            }

            if (activeTriangles.Count == 0)
                return null;

            return gpuContext.TryRenderBatch(
                activeTriangles,
                sliceHeightsMm,
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight);
        }
        catch (Exception ex)
        {
            logger?.LogDebug(ex, "GPU slice projection unavailable for this batch; using CPU fallback.");
            return null;
        }
    }

    private static List<Triangle3D> BuildBatchTriangles(
        IReadOnlyList<IReadOnlyList<Triangle3D>> trianglesByLayer,
        float sliceMinMm,
        float sliceMaxMm,
        int maxTriangleCount)
    {
        var uniqueTriangles = new HashSet<Triangle3D>();

        foreach (var triangles in trianglesByLayer)
        {
            foreach (var triangle in triangles)
            {
                if (!IntersectsSliceRange(triangle, sliceMinMm, sliceMaxMm))
                    continue;

                uniqueTriangles.Add(triangle);
                if (uniqueTriangles.Count > maxTriangleCount)
                    return [];
            }
        }

        return uniqueTriangles.ToList();
    }

    private static SliceBitmap RenderLayerBitmapCpu(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm)
    {
        var bitmap = new SliceBitmap(pixelWidth, pixelHeight);
        var candidatesByRow = BuildRowCandidates(triangles, sliceHeightMm, layerThicknessMm, bedDepthMm, pixelHeight);

        Parallel.For(0, pixelHeight, row =>
        {
            var candidates = candidatesByRow[row];
            if (candidates == null || candidates.Count == 0)
                return;

            var zMm = RowToZ(row, bedDepthMm, pixelHeight);
            var span = bitmap.GetRowSpan(row);
            FillProjectedRow(span, triangles, candidates, sliceHeightMm, zMm, bedWidthMm, pixelWidth);
        });

        bitmap.RemoveUnsupportedHorizontalPixels();
        return bitmap;
    }

    private static SliceBitmap RenderLayerBitmapCpuSerial(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm)
    {
        var bitmap = new SliceBitmap(pixelWidth, pixelHeight);
        var candidatesByRow = BuildRowCandidates(triangles, sliceHeightMm, layerThicknessMm, bedDepthMm, pixelHeight);

        for (var row = 0; row < pixelHeight; row++)
        {
            var candidates = candidatesByRow[row];
            if (candidates == null || candidates.Count == 0)
                continue;

            var zMm = RowToZ(row, bedDepthMm, pixelHeight);
            var span = bitmap.GetRowSpan(row);
            FillProjectedRow(span, triangles, candidates, sliceHeightMm, zMm, bedWidthMm, pixelWidth);
        }

        bitmap.RemoveUnsupportedHorizontalPixels();
        return bitmap;
    }

    private static List<Triangle3D> FilterActiveTrianglesForGpu(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float layerThicknessMm,
        int maxTriangleCount)
    {
        var sliceMinMm = sliceHeightMm - (layerThicknessMm * 0.5f);
        var sliceMaxMm = sliceHeightMm + (layerThicknessMm * 0.5f);
        var active = new List<Triangle3D>(Math.Min(triangles.Count, maxTriangleCount));

        foreach (var triangle in triangles)
        {
            if (!IntersectsSliceRange(triangle, sliceMinMm, sliceMaxMm))
                continue;

            active.Add(triangle);
            if (active.Count > maxTriangleCount)
                return [];
        }

        return active;
    }

    private static bool IntersectsSliceRange(Triangle3D triangle, float sliceMinMm, float sliceMaxMm)
    {
        var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
        var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
        return !(sliceMaxMm < minY - Epsilon || sliceMinMm > maxY + Epsilon);
    }

    private static List<int>?[] BuildRowCandidates(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float layerThicknessMm,
        float bedDepthMm,
        int pixelHeight)
    {
        var sliceMinMm = sliceHeightMm - (layerThicknessMm * 0.5f);
        var sliceMaxMm = sliceHeightMm + (layerThicknessMm * 0.5f);
        var byRow = new List<int>?[pixelHeight];

        for (var index = 0; index < triangles.Count; index++)
        {
            var triangle = triangles[index];
            if (!IntersectsSliceRange(triangle, sliceMinMm, sliceMaxMm))
                continue;

            var minZ = MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z));
            var maxZ = MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z));
            var startRow = Math.Clamp(MapZToRow(maxZ, bedDepthMm, pixelHeight) - 1, 0, pixelHeight - 1);
            var endRow = Math.Clamp(MapZToRow(minZ, bedDepthMm, pixelHeight) + 1, 0, pixelHeight - 1);

            if (endRow < startRow)
                (startRow, endRow) = (endRow, startRow);

            for (var row = startRow; row <= endRow; row++)
            {
                byRow[row] ??= [];
                byRow[row]!.Add(index);
            }
        }

        return byRow;
    }

    private static void FillProjectedRow(
        Span<byte> span,
        IReadOnlyList<Triangle3D> triangles,
        List<int> candidateIndexes,
        float sliceHeightMm,
        float zMm,
        float bedWidthMm,
        int pixelWidth)
    {
        var origin = new Vec3((-bedWidthMm * 0.5f) - RayOffsetMm, sliceHeightMm, zMm);
        Span<float> hitXs = stackalloc float[64];
        Span<int> hitDeltas = stackalloc int[64];
        List<RayHit>? overflowHits = null;
        var hitCount = 0;

        foreach (var triangleIndex in candidateIndexes)
        {
            var triangle = triangles[triangleIndex];
            if (!TryIntersectPositiveXRay(origin, triangle, out var hitX))
                continue;

            var hitDelta = GetWindingDelta(triangle);
            if (hitDelta == 0)
                continue;

            if (TryAccumulateHit(hitXs, hitDeltas, hitCount, hitX, hitDelta))
                continue;

            if (overflowHits is not null)
            {
                AddOrAccumulateHit(overflowHits, hitX, hitDelta);
                continue;
            }

            if (hitCount < hitXs.Length)
            {
                hitXs[hitCount] = hitX;
                hitDeltas[hitCount] = hitDelta;
                hitCount++;
                continue;
            }

            overflowHits = new List<RayHit>(hitCount + 8);
            for (var i = 0; i < hitCount; i++)
                overflowHits.Add(new RayHit(hitXs[i], hitDeltas[i]));

            AddOrAccumulateHit(overflowHits, hitX, hitDelta);
        }

        if (overflowHits is not null)
        {
            overflowHits.Sort(static (a, b) => a.X.CompareTo(b.X));
            FillWindingIntervals(span, CollectionsMarshal.AsSpan(overflowHits), bedWidthMm, pixelWidth);
            return;
        }

        SortHits(hitXs, hitDeltas, hitCount);
        FillWindingIntervals(span, hitXs, hitDeltas, hitCount, bedWidthMm, pixelWidth);
    }

    private static bool TryAccumulateHit(ReadOnlySpan<float> hitXs, Span<int> hitDeltas, int count, float candidate, int delta)
    {
        for (var i = 0; i < count; i++)
        {
            if (MathF.Abs(hitXs[i] - candidate) > HitDedupEpsilon)
                continue;

            hitDeltas[i] += delta;
            return true;
        }

        return false;
    }

    private static void AddOrAccumulateHit(List<RayHit> hits, float candidate, int delta)
    {
        for (var i = 0; i < hits.Count; i++)
        {
            if (MathF.Abs(hits[i].X - candidate) > HitDedupEpsilon)
                continue;

            hits[i] = hits[i] with { Delta = hits[i].Delta + delta };
            return;
        }

        hits.Add(new RayHit(candidate, delta));
    }

    private static int GetWindingDelta(Triangle3D triangle)
    {
        var normalX = (triangle.V1 - triangle.V0).Cross(triangle.V2 - triangle.V0).X;
        if (MathF.Abs(normalX) <= Epsilon)
            return 0;

        return normalX < 0f ? 1 : -1;
    }

    private static void SortHits(Span<float> hitXs, Span<int> hitDeltas, int count)
    {
        for (var i = 1; i < count; i++)
        {
            var x = hitXs[i];
            var delta = hitDeltas[i];
            var j = i - 1;

            while (j >= 0 && hitXs[j] > x)
            {
                hitXs[j + 1] = hitXs[j];
                hitDeltas[j + 1] = hitDeltas[j];
                j--;
            }

            hitXs[j + 1] = x;
            hitDeltas[j + 1] = delta;
        }
    }

    private static void FillWindingIntervals(
        Span<byte> span,
        ReadOnlySpan<float> hitXs,
        ReadOnlySpan<int> hitDeltas,
        int count,
        float bedWidthMm,
        int pixelWidth)
    {
        float? startX = null;
        var winding = 0;

        for (var i = 0; i < count; i++)
        {
            if (hitDeltas[i] == 0)
                continue;

            var previousWinding = winding;
            winding += hitDeltas[i];

            if (previousWinding == 0 && winding != 0)
            {
                startX = hitXs[i];
                continue;
            }

            if (previousWinding != 0 && winding == 0 && startX.HasValue)
            {
                FillInterval(span, startX.Value, hitXs[i], bedWidthMm, pixelWidth);
                startX = null;
            }
        }
    }

    private static void FillWindingIntervals(Span<byte> span, ReadOnlySpan<RayHit> hits, float bedWidthMm, int pixelWidth)
    {
        float? startX = null;
        var winding = 0;

        foreach (var hit in hits)
        {
            if (hit.Delta == 0)
                continue;

            var previousWinding = winding;
            winding += hit.Delta;

            if (previousWinding == 0 && winding != 0)
            {
                startX = hit.X;
                continue;
            }

            if (previousWinding != 0 && winding == 0 && startX.HasValue)
            {
                FillInterval(span, startX.Value, hit.X, bedWidthMm, pixelWidth);
                startX = null;
            }
        }
    }

    private static void FillInterval(Span<byte> span, float startX, float endX, float bedWidthMm, int pixelWidth)
    {
        if (MathF.Abs(startX - endX) <= HitDedupEpsilon)
            return;

        var bedHalfWidth = bedWidthMm * 0.5f;
        if ((startX < -bedHalfWidth - HitDedupEpsilon && endX < -bedHalfWidth - HitDedupEpsilon)
            || (startX > bedHalfWidth + HitDedupEpsilon && endX > bedHalfWidth + HitDedupEpsilon))
            return;

        var minX = Math.Clamp(MathF.Min(startX, endX), -bedHalfWidth, bedHalfWidth);
        var maxX = Math.Clamp(MathF.Max(startX, endX), -bedHalfWidth, bedHalfWidth);
        var start = Math.Clamp((int)MathF.Floor(((minX + bedHalfWidth) / bedWidthMm) * pixelWidth), 0, pixelWidth - 1);
        var endExclusive = Math.Clamp((int)MathF.Ceiling(((maxX + bedHalfWidth) / bedWidthMm) * pixelWidth), 0, pixelWidth);
        var end = Math.Clamp(endExclusive - 1, 0, pixelWidth - 1);
        if (end < start)
            return;

        span.Slice(start, (end - start) + 1).Fill((byte)255);
    }

    private readonly record struct RayHit(float X, int Delta);

    private static bool TryIntersectPositiveXRay(Vec3 origin, Triangle3D triangle, out float hitX)
    {
        var maxX = MathF.Max(triangle.V0.X, MathF.Max(triangle.V1.X, triangle.V2.X));
        if (maxX < origin.X + Epsilon)
        {
            hitX = 0f;
            return false;
        }

        var minZ = MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z));
        var maxZ = MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z));
        if (origin.Z < minZ - Epsilon || origin.Z > maxZ + Epsilon)
        {
            hitX = 0f;
            return false;
        }

        var edge1 = triangle.V1 - triangle.V0;
        var edge2 = triangle.V2 - triangle.V0;
        var h = RayDirection.Cross(edge2);
        var a = edge1.Dot(h);

        if (MathF.Abs(a) < Epsilon)
        {
            hitX = 0f;
            return false;
        }

        var invA = 1f / a;
        var s = origin - triangle.V0;
        var u = invA * s.Dot(h);
        if (u < -Epsilon || u > 1f + Epsilon)
        {
            hitX = 0f;
            return false;
        }

        var q = s.Cross(edge1);
        var v = invA * RayDirection.Dot(q);
        if (v < -Epsilon || u + v > 1f + Epsilon)
        {
            hitX = 0f;
            return false;
        }

        var t = invA * edge2.Dot(q);
        if (t < Epsilon)
        {
            hitX = 0f;
            return false;
        }

        hitX = origin.X + t;
        return true;
    }

    private static int MapXToColumn(float xMm, float bedWidthMm, int width)
        => (int)MathF.Floor(((xMm + (bedWidthMm * 0.5f)) / bedWidthMm) * width);

    private static int MapZToRow(float zMm, float bedDepthMm, int height)
        => (int)MathF.Floor((((bedDepthMm * 0.5f) - zMm) / bedDepthMm) * height);

    private static float RowToZ(int row, float bedDepthMm, int height)
        => (bedDepthMm * 0.5f) - (((row + 0.5f) / height) * bedDepthMm);
}
