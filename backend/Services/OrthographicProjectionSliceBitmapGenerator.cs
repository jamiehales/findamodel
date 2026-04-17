using System.Runtime.InteropServices;
using Microsoft.Extensions.Logging;

namespace findamodel.Services;

public sealed class OrthographicProjectionSliceBitmapGenerator : IPlateSliceBitmapGenerator
{
    private const float Epsilon = 0.0001f;
    private const float RayOffsetMm = 0.0005f;
    private const float HitDedupEpsilon = 0.0005f;
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
        if (gpuContext is not null && gpuContext.IsAvailable)
        {
            try
            {
                var activeTriangles = FilterActiveTrianglesForGpu(triangles, sliceHeightMm, maxTriangleCount: 250_000);
                if (activeTriangles.Count > 0)
                {
                    var gpuBitmap = gpuContext.TryRender(
                        activeTriangles,
                        sliceHeightMm,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight);

                    if (gpuBitmap is not null)
                        return gpuBitmap;
                }
            }
            catch (Exception ex)
            {
                logger?.LogDebug(ex, "GPU slice projection unavailable for this layer; using CPU fallback.");
            }
        }

        var bitmap = new SliceBitmap(pixelWidth, pixelHeight);
        var candidatesByRow = BuildRowCandidates(triangles, sliceHeightMm, bedDepthMm, pixelHeight);

        Parallel.For(0, pixelHeight, row =>
        {
            var candidates = candidatesByRow[row];
            if (candidates == null || candidates.Count == 0)
                return;

            var zMm = RowToZ(row, bedDepthMm, pixelHeight);
            var span = bitmap.GetRowSpan(row);
            FillProjectedRow(span, triangles, candidates, sliceHeightMm, zMm, bedWidthMm, pixelWidth);
        });

        return bitmap;
    }

    private static List<Triangle3D> FilterActiveTrianglesForGpu(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        int maxTriangleCount)
    {
        var active = new List<Triangle3D>();
        foreach (var triangle in triangles)
        {
            var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
            var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            if (sliceHeightMm < minY - Epsilon || sliceHeightMm > maxY + Epsilon)
                continue;

            active.Add(triangle);
            if (active.Count > maxTriangleCount)
                return [];
        }

        return active;
    }

    private static List<int>?[] BuildRowCandidates(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedDepthMm,
        int pixelHeight)
    {
        var byRow = new List<int>?[pixelHeight];
        for (var index = 0; index < triangles.Count; index++)
        {
            var triangle = triangles[index];
            var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
            var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            if (sliceHeightMm < minY - Epsilon || sliceHeightMm > maxY + Epsilon)
                continue;

            var minZ = MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z));
            var maxZ = MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z));
            var startRow = Math.Clamp(MapZToRow(maxZ, bedDepthMm, pixelHeight), 0, pixelHeight - 1);
            var endRow = Math.Clamp(MapZToRow(minZ, bedDepthMm, pixelHeight), 0, pixelHeight - 1);

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
        List<float>? overflowHits = null;
        var uniqueHitCount = 0;

        foreach (var triangleIndex in candidateIndexes)
        {
            if (!TryIntersectPositiveXRay(origin, triangles[triangleIndex], out var hitX))
                continue;

            if (ContainsApproximately(hitXs, uniqueHitCount, hitX) || (overflowHits is not null && ContainsApproximately(CollectionsMarshal.AsSpan(overflowHits), overflowHits.Count, hitX)))
                continue;

            if (uniqueHitCount < hitXs.Length)
            {
                hitXs[uniqueHitCount++] = hitX;
                continue;
            }

            overflowHits ??= hitXs.Slice(0, uniqueHitCount).ToArray().ToList();
            overflowHits.Add(hitX);
        }

        if (overflowHits is not null)
        {
            overflowHits.Sort();
            FillIntervals(span, CollectionsMarshal.AsSpan(overflowHits), bedWidthMm, pixelWidth);
            return;
        }

        var sortedHits = hitXs.Slice(0, uniqueHitCount);
        sortedHits.Sort();
        FillIntervals(span, sortedHits, bedWidthMm, pixelWidth);
    }

    private static bool ContainsApproximately(ReadOnlySpan<float> values, int count, float candidate)
    {
        for (var i = 0; i < count; i++)
        {
            if (MathF.Abs(values[i] - candidate) <= HitDedupEpsilon)
                return true;
        }

        return false;
    }

    private static void FillIntervals(Span<byte> span, ReadOnlySpan<float> hits, float bedWidthMm, int pixelWidth)
    {
        float? startX = null;
        foreach (var hit in hits)
        {
            if (!startX.HasValue)
            {
                startX = hit;
                continue;
            }

            if (MathF.Abs(startX.Value - hit) <= HitDedupEpsilon)
                continue;

            var start = Math.Clamp(MapXToColumn(startX.Value, bedWidthMm, pixelWidth), 0, pixelWidth - 1);
            var end = Math.Clamp(MapXToColumn(hit, bedWidthMm, pixelWidth), 0, pixelWidth - 1);
            if (end < start)
                (start, end) = (end, start);

            span.Slice(start, (end - start) + 1).Fill((byte)255);
            startX = null;
        }
    }

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
