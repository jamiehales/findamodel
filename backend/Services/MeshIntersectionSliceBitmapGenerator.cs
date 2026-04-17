namespace findamodel.Services;

public sealed class MeshIntersectionSliceBitmapGenerator : IPlateSliceBitmapGenerator
{
    private const float Epsilon = 0.0001f;

    public PngSliceExportMethod Method => PngSliceExportMethod.MeshIntersection;

    public SliceBitmap RenderLayerBitmap(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm = PlateSliceRasterService.DefaultLayerHeightMm)
    {
        var bitmap = new SliceBitmap(pixelWidth, pixelHeight);
        var rowIntersections = new List<float>?[pixelHeight];

        foreach (var triangle in triangles)
        {
            if (!TryIntersectTriangle(triangle, sliceHeightMm, out var p0, out var p1))
                continue;

            AddSegmentIntersections(p0, p1, rowIntersections, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight);
        }

        for (var row = 0; row < pixelHeight; row++)
        {
            var xs = rowIntersections[row];
            if (xs == null || xs.Count < 2)
                continue;

            xs.Sort();
            var normalized = NormalizeIntersections(xs);
            var span = bitmap.GetRowSpan(row);

            for (var i = 0; i + 1 < normalized.Count; i += 2)
            {
                var start = Math.Clamp(MapXToColumn(normalized[i], bedWidthMm, pixelWidth), 0, pixelWidth - 1);
                var end = Math.Clamp(MapXToColumn(normalized[i + 1], bedWidthMm, pixelWidth), 0, pixelWidth - 1);

                if (end < start)
                    (start, end) = (end, start);

                span.Slice(start, (end - start) + 1).Fill((byte)255);
            }
        }

        return bitmap;
    }

    private static List<float> NormalizeIntersections(List<float> xs)
    {
        var result = new List<float>(xs.Count);
        foreach (var x in xs)
        {
            if (result.Count == 0 || MathF.Abs(result[^1] - x) > Epsilon)
                result.Add(x);
        }

        return result;
    }

    private static void AddSegmentIntersections(
        (float X, float Z) p0,
        (float X, float Z) p1,
        List<float>?[] rowIntersections,
        float bedWidthMm,
        float bedDepthMm,
        int width,
        int height)
    {
        if (MathF.Abs(p1.Z - p0.Z) < Epsilon)
            return;

        var minZ = MathF.Min(p0.Z, p1.Z);
        var maxZ = MathF.Max(p0.Z, p1.Z);
        var startRow = Math.Clamp(MapZToRow(maxZ, bedDepthMm, height), 0, height - 1);
        var endRow = Math.Clamp(MapZToRow(minZ, bedDepthMm, height), 0, height - 1);

        if (endRow < startRow)
            (startRow, endRow) = (endRow, startRow);

        for (var row = startRow; row <= endRow; row++)
        {
            var zMm = RowToZ(row, bedDepthMm, height);
            if (zMm < minZ - Epsilon || zMm > maxZ + Epsilon)
                continue;

            var t = (zMm - p0.Z) / (p1.Z - p0.Z);
            if (t < -Epsilon || t > 1f + Epsilon)
                continue;

            var xMm = p0.X + ((p1.X - p0.X) * t);
            if (xMm < (-bedWidthMm * 0.5f) - Epsilon || xMm > (bedWidthMm * 0.5f) + Epsilon)
                continue;

            rowIntersections[row] ??= [];
            rowIntersections[row]!.Add(xMm);
        }
    }

    private static bool TryIntersectTriangle(
        Triangle3D triangle,
        float sliceHeight,
        out (float X, float Z) p0,
        out (float X, float Z) p1)
    {
        var intersections = new List<(float X, float Z)>(6);
        TryIntersectEdge(triangle.V0, triangle.V1, sliceHeight, intersections);
        TryIntersectEdge(triangle.V1, triangle.V2, sliceHeight, intersections);
        TryIntersectEdge(triangle.V2, triangle.V0, sliceHeight, intersections);

        var distinct = intersections
            .DistinctBy(p => (MathF.Round(p.X, 4), MathF.Round(p.Z, 4)))
            .ToArray();

        if (distinct.Length < 2)
        {
            p0 = default;
            p1 = default;
            return false;
        }

        var bestA = distinct[0];
        var bestB = distinct[1];
        var bestDistance = DistanceSquared(bestA, bestB);

        for (var i = 0; i < distinct.Length; i++)
        {
            for (var j = i + 1; j < distinct.Length; j++)
            {
                var distance = DistanceSquared(distinct[i], distinct[j]);
                if (distance > bestDistance)
                {
                    bestDistance = distance;
                    bestA = distinct[i];
                    bestB = distinct[j];
                }
            }
        }

        p0 = bestA;
        p1 = bestB;
        return true;
    }

    private static void TryIntersectEdge(Vec3 a, Vec3 b, float sliceHeight, List<(float X, float Z)> intersections)
    {
        if (MathF.Abs(a.Y - b.Y) < Epsilon)
            return;

        var crosses = (a.Y < sliceHeight && b.Y >= sliceHeight) || (b.Y < sliceHeight && a.Y >= sliceHeight);
        if (!crosses)
            return;

        var t = (sliceHeight - a.Y) / (b.Y - a.Y);
        intersections.Add((a.X + ((b.X - a.X) * t), a.Z + ((b.Z - a.Z) * t)));
    }

    private static float DistanceSquared((float X, float Z) a, (float X, float Z) b)
    {
        var dx = a.X - b.X;
        var dz = a.Z - b.Z;
        return (dx * dx) + (dz * dz);
    }

    private static int MapXToColumn(float xMm, float bedWidthMm, int width)
        => (int)MathF.Floor(((xMm + (bedWidthMm * 0.5f)) / bedWidthMm) * width);

    private static int MapZToRow(float zMm, float bedDepthMm, int height)
        => (int)MathF.Floor((((bedDepthMm * 0.5f) - zMm) / bedDepthMm) * height);

    private static float RowToZ(int row, float bedDepthMm, int height)
        => (bedDepthMm * 0.5f) - (((row + 0.5f) / height) * bedDepthMm);
}
