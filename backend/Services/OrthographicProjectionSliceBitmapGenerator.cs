namespace findamodel.Services;

public sealed class OrthographicProjectionSliceBitmapGenerator : IPlateSliceBitmapGenerator
{
    private const float Epsilon = 0.0001f;
    private const float RayOffsetMm = 0.0005f;

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
        var bitmap = new SliceBitmap(pixelWidth, pixelHeight);
        var candidatesByRow = BuildRowCandidates(triangles, sliceHeightMm, bedDepthMm, pixelHeight);

        for (var row = 0; row < pixelHeight; row++)
        {
            var candidates = candidatesByRow[row];
            if (candidates == null || candidates.Count == 0)
                continue;

            var zMm = RowToZ(row, bedDepthMm, pixelHeight);
            var span = bitmap.GetRowSpan(row);

            for (var column = 0; column < pixelWidth; column++)
            {
                var xMm = ColumnToX(column, bedWidthMm, pixelWidth);
                if (IsPointInsideMesh(new Vec3(xMm - RayOffsetMm, sliceHeightMm, zMm), candidates))
                    span[column] = 255;
            }
        }

        return bitmap;
    }

    private static List<Triangle3D>?[] BuildRowCandidates(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedDepthMm,
        int pixelHeight)
    {
        var byRow = new List<Triangle3D>?[pixelHeight];
        foreach (var triangle in triangles)
        {
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
                byRow[row]!.Add(triangle);
            }
        }

        return byRow;
    }

    private static bool IsPointInsideMesh(Vec3 origin, List<Triangle3D> candidates)
    {
        var hitXs = new List<float>();
        foreach (var triangle in candidates)
        {
            if (TryIntersectPositiveXRay(origin, triangle, out var hitX))
                hitXs.Add(hitX);
        }

        if (hitXs.Count == 0)
            return false;

        hitXs.Sort();
        var uniqueHitCount = 0;
        float? last = null;
        foreach (var hit in hitXs)
        {
            if (!last.HasValue || MathF.Abs(hit - last.Value) > 0.0005f)
            {
                uniqueHitCount++;
                last = hit;
            }
        }

        return (uniqueHitCount % 2) == 1;
    }

    private static bool TryIntersectPositiveXRay(Vec3 origin, Triangle3D triangle, out float hitX)
    {
        var direction = new Vec3(1f, 0f, 0f);
        var edge1 = triangle.V1 - triangle.V0;
        var edge2 = triangle.V2 - triangle.V0;
        var h = direction.Cross(edge2);
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
        var v = invA * direction.Dot(q);
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

    private static float ColumnToX(int column, float bedWidthMm, int width)
        => (((column + 0.5f) / width) * bedWidthMm) - (bedWidthMm * 0.5f);

    private static int MapZToRow(float zMm, float bedDepthMm, int height)
        => (int)MathF.Floor((((bedDepthMm * 0.5f) - zMm) / bedDepthMm) * height);

    private static float RowToZ(int row, float bedDepthMm, int height)
        => (bedDepthMm * 0.5f) - (((row + 0.5f) / height) * bedDepthMm);
}
