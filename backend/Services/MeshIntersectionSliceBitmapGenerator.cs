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
        var rowIntersections = new List<ScanlineIntersection>?[pixelHeight];

        foreach (var triangle in triangles)
        {
            var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
            var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            if (sliceHeightMm < minY - Epsilon || sliceHeightMm > maxY + Epsilon)
                continue;

            if (!TryIntersectTriangle(triangle, sliceHeightMm, out var p0, out var p1))
                continue;

            OrientSegmentForWinding(triangle, ref p0, ref p1);
            AddSegmentIntersections(p0, p1, rowIntersections, bedWidthMm, bedDepthMm, pixelHeight);
        }

        for (var row = 0; row < pixelHeight; row++)
        {
            var intersections = rowIntersections[row];
            if (intersections == null || intersections.Count < 2)
                continue;

            intersections.Sort(static (a, b) => a.X.CompareTo(b.X));
            var span = bitmap.GetRowSpan(row);
            FillScanline(intersections, span, bedWidthMm, pixelWidth);
        }

        bitmap.RemoveUnsupportedHorizontalPixels();
        return bitmap;
    }

    private static void FillScanline(List<ScanlineIntersection> intersections, Span<byte> span, float bedWidthMm, int pixelWidth)
    {
        float? startX = null;
        var winding = 0;
        var index = 0;

        while (index < intersections.Count)
        {
            var currentX = intersections[index].X;
            var deltaSum = 0;
            while (index < intersections.Count && MathF.Abs(intersections[index].X - currentX) <= 0.002f)
            {
                deltaSum += intersections[index].WindingDelta;
                index++;
            }

            if (deltaSum == 0)
                continue;

            var previousWinding = winding;
            winding += deltaSum;

            if (previousWinding == 0 && winding != 0)
            {
                startX = currentX;
                continue;
            }

            if (previousWinding != 0 && winding == 0 && startX.HasValue)
            {
                FillInterval(span, startX.Value, currentX, bedWidthMm, pixelWidth);
                startX = null;
            }
        }
    }

    private static void AddSegmentIntersections(
        (float X, float Z) p0,
        (float X, float Z) p1,
        List<ScanlineIntersection>?[] rowIntersections,
        float bedWidthMm,
        float bedDepthMm,
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

        var windingDelta = p0.Z < p1.Z ? 1 : -1;

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
            rowIntersections[row]!.Add(new ScanlineIntersection(xMm, windingDelta));
        }
    }

    private static bool TryIntersectTriangle(
        Triangle3D triangle,
        float sliceHeight,
        out (float X, float Z) p0,
        out (float X, float Z) p1)
    {
        Span<PointXZ> intersections = stackalloc PointXZ[6];
        var count = 0;

        TryIntersectEdge(triangle.V0, triangle.V1, sliceHeight, intersections, ref count);
        TryIntersectEdge(triangle.V1, triangle.V2, sliceHeight, intersections, ref count);
        TryIntersectEdge(triangle.V2, triangle.V0, sliceHeight, intersections, ref count);

        if (count < 2)
        {
            p0 = default;
            p1 = default;
            return false;
        }

        var distinctCount = DeduplicateIntersections(intersections, count);
        if (distinctCount < 2)
        {
            p0 = default;
            p1 = default;
            return false;
        }

        var bestA = intersections[0];
        var bestB = intersections[1];
        var bestDistance = DistanceSquared(bestA, bestB);

        for (var i = 0; i < distinctCount; i++)
        {
            for (var j = i + 1; j < distinctCount; j++)
            {
                var distance = DistanceSquared(intersections[i], intersections[j]);
                if (distance > bestDistance)
                {
                    bestDistance = distance;
                    bestA = intersections[i];
                    bestB = intersections[j];
                }
            }
        }

        p0 = (bestA.X, bestA.Z);
        p1 = (bestB.X, bestB.Z);
        return true;
    }

    private static void TryIntersectEdge(
        Vec3 a,
        Vec3 b,
        float sliceHeight,
        Span<PointXZ> intersections,
        ref int count)
    {
        if (MathF.Abs(a.Y - b.Y) < Epsilon)
            return;

        var crosses = (a.Y < sliceHeight && b.Y >= sliceHeight) || (b.Y < sliceHeight && a.Y >= sliceHeight);
        if (!crosses)
            return;

        var t = (sliceHeight - a.Y) / (b.Y - a.Y);
        intersections[count++] = new PointXZ(
            a.X + ((b.X - a.X) * t),
            a.Z + ((b.Z - a.Z) * t));
    }

    private static int DeduplicateIntersections(Span<PointXZ> intersections, int count)
    {
        var distinctCount = 0;
        for (var i = 0; i < count; i++)
        {
            var candidate = intersections[i];
            var duplicate = false;
            for (var j = 0; j < distinctCount; j++)
            {
                if (MathF.Abs(intersections[j].X - candidate.X) <= Epsilon
                    && MathF.Abs(intersections[j].Z - candidate.Z) <= Epsilon)
                {
                    duplicate = true;
                    break;
                }
            }

            if (!duplicate)
                intersections[distinctCount++] = candidate;
        }

        return distinctCount;
    }

    private static void OrientSegmentForWinding(Triangle3D triangle, ref (float X, float Z) p0, ref (float X, float Z) p1)
    {
        var normal = (triangle.V1 - triangle.V0).Cross(triangle.V2 - triangle.V0);
        var expectedDirection = Vec3.Up.Cross(normal);
        var dx = p1.X - p0.X;
        var dz = p1.Z - p0.Z;
        if ((dx * expectedDirection.X) + (dz * expectedDirection.Z) < 0f)
            (p0, p1) = (p1, p0);
    }

    private static void FillInterval(Span<byte> span, float startX, float endX, float bedWidthMm, int pixelWidth)
    {
        if (MathF.Abs(startX - endX) <= Epsilon)
            return;

        var bedHalfWidth = bedWidthMm * 0.5f;
        if ((startX < -bedHalfWidth - Epsilon && endX < -bedHalfWidth - Epsilon)
            || (startX > bedHalfWidth + Epsilon && endX > bedHalfWidth + Epsilon))
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

    private static float DistanceSquared(PointXZ a, PointXZ b)
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

    private readonly record struct PointXZ(float X, float Z);
    private readonly record struct ScanlineIntersection(float X, int WindingDelta);
}
