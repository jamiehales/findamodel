using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class SliceBitmapHarnessTests
{
    private const float BedWidthMm = 30f;
    private const float BedDepthMm = 30f;
    private const int PixelWidth = 180;
    private const int PixelHeight = 180;

    public static TheoryData<PngSliceExportMethod> Methods =>
    [
        PngSliceExportMethod.MeshIntersection,
        PngSliceExportMethod.OrthographicProjection,
    ];

    [Theory]
    [MemberData(nameof(Methods))]
    public void CubeSlice_MatchesAnalyticBitmap(PngSliceExportMethod method)
    {
        const float sideMm = 10f;
        const float sliceHeightMm = 5f;
        var generator = CreateGenerator(method);

        var actual = generator.RenderLayerBitmap(
            CreateCube(sideMm),
            sliceHeightMm,
            BedWidthMm,
            BedDepthMm,
            PixelWidth,
            PixelHeight);

        var half = sideMm * 0.5f;
        var expected = BuildExpectedBitmap((x, z) => MathF.Abs(x) <= half && MathF.Abs(z) <= half);

        AssertBitmapsMatch(expected, actual, minIou: 0.96f, maxAreaErrorRatio: 0.06f);
    }

    [Theory]
    [MemberData(nameof(Methods))]
    public void CylinderSlice_MatchesAnalyticBitmap(PngSliceExportMethod method)
    {
        const float radiusMm = 5f;
        const float heightMm = 12f;
        const float sliceHeightMm = 6f;
        var generator = CreateGenerator(method);

        var actual = generator.RenderLayerBitmap(
            CreateCylinder(radiusMm, heightMm, segments: 96),
            sliceHeightMm,
            BedWidthMm,
            BedDepthMm,
            PixelWidth,
            PixelHeight);

        var expected = BuildExpectedBitmap((x, z) => (x * x) + (z * z) <= (radiusMm * radiusMm));

        AssertBitmapsMatch(expected, actual, minIou: 0.94f, maxAreaErrorRatio: 0.08f);
    }

    [Theory]
    [MemberData(nameof(Methods))]
    public void SphereSlice_MatchesAnalyticBitmap(PngSliceExportMethod method)
    {
        const float radiusMm = 6f;
        const float sliceHeightMm = 3.5f;
        var generator = CreateGenerator(method);

        var actual = generator.RenderLayerBitmap(
            CreateSphere(radiusMm, latSegments: 32, lonSegments: 64),
            sliceHeightMm,
            BedWidthMm,
            BedDepthMm,
            PixelWidth,
            PixelHeight);

        var dy = sliceHeightMm - radiusMm;
        var sliceRadiusSq = (radiusMm * radiusMm) - (dy * dy);
        var expected = BuildExpectedBitmap((x, z) => (x * x) + (z * z) <= sliceRadiusSq);

        AssertBitmapsMatch(expected, actual, minIou: 0.90f, maxAreaErrorRatio: 0.12f);
    }

    [Theory]
    [MemberData(nameof(Methods))]
    public void PyramidSlice_MatchesAnalyticBitmap(PngSliceExportMethod method)
    {
        const float baseRadiusMm = 6f;
        const float heightMm = 12f;
        const float sliceHeightMm = 4f;
        var generator = CreateGenerator(method);

        var actual = generator.RenderLayerBitmap(
            CreateTriangularPyramid(baseRadiusMm, heightMm),
            sliceHeightMm,
            BedWidthMm,
            BedDepthMm,
            PixelWidth,
            PixelHeight);

        var scale = 1f - (sliceHeightMm / heightMm);
        var baseTriangle = GetBaseTriangle(baseRadiusMm);
        var expectedA = (baseTriangle.A.X * scale, baseTriangle.A.Z * scale);
        var expectedB = (baseTriangle.B.X * scale, baseTriangle.B.Z * scale);
        var expectedC = (baseTriangle.C.X * scale, baseTriangle.C.Z * scale);
        var expected = BuildExpectedBitmap((x, z) => PointInTriangle((x, z), expectedA, expectedB, expectedC));

        AssertBitmapsMatch(expected, actual, minIou: 0.90f, maxAreaErrorRatio: 0.12f);
    }

    [Theory]
    [MemberData(nameof(Methods))]
    public void OverlappingCubesSlice_FillsUnionInsteadOfParityGaps(PngSliceExportMethod method)
    {
        const float sideMm = 8f;
        const float sliceHeightMm = 4f;
        var generator = CreateGenerator(method);

        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateOffsetCube(sideMm, centerX: -2f, centerZ: 0f));
        triangles.AddRange(CreateOffsetCube(sideMm, centerX: 2f, centerZ: 0f));

        var actual = generator.RenderLayerBitmap(
            triangles,
            sliceHeightMm,
            BedWidthMm,
            BedDepthMm,
            PixelWidth,
            PixelHeight);

        var half = sideMm * 0.5f;
        var expected = BuildExpectedBitmap((x, z) =>
            PointInAxisAlignedSquare(x, z, centerX: -2f, centerZ: 0f, half)
            || PointInAxisAlignedSquare(x, z, centerX: 2f, centerZ: 0f, half));

        AssertBitmapsMatch(expected, actual, minIou: 0.96f, maxAreaErrorRatio: 0.06f);
    }

    [Theory]
    [MemberData(nameof(Methods))]
    public void SeparatedCubesSlice_DoesNotCreateHorizontalBridgeRows(PngSliceExportMethod method)
    {
        const float sideMm = 5f;
        const float sliceHeightMm = 2.5f;
        var generator = CreateGenerator(method);

        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateOffsetCube(sideMm, centerX: -5f, centerZ: 0f));
        triangles.AddRange(CreateOffsetCube(sideMm, centerX: 5f, centerZ: 0f));

        var actual = generator.RenderLayerBitmap(
            triangles,
            sliceHeightMm,
            bedWidthMm: 24f,
            bedDepthMm: 10f,
            pixelWidth: 96,
            pixelHeight: 20);

        var rowsWithPixels = 0;
        for (var row = 0; row < actual.Height; row++)
        {
            var runs = CountLitRuns(actual.GetRowSpan(row));
            if (runs.Count == 0)
                continue;

            rowsWithPixels++;
            Assert.Equal(2, runs.Count);
            Assert.All(runs, run => Assert.InRange(run, 16, 24));
        }

        Assert.True(rowsWithPixels > 0);
    }

    [Theory]
    [MemberData(nameof(Methods))]
    public void NarrowSeparatedBoxesSlice_PreservesDistinctConnectedIslands(PngSliceExportMethod method)
    {
        var generator = CreateGenerator(method);
        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateOffsetBox(widthMm: 1.2f, depthMm: 2f, heightMm: 4f, centerX: -2.5f, centerZ: 0f));
        triangles.AddRange(CreateOffsetBox(widthMm: 1.2f, depthMm: 2f, heightMm: 4f, centerX: 2.5f, centerZ: 0f));

        var actual = generator.RenderLayerBitmap(
            triangles,
            sliceHeightMm: 1.25f,
            bedWidthMm: 60f,
            bedDepthMm: 40f,
            pixelWidth: 72,
            pixelHeight: 48);

        Assert.Equal(2, CountConnectedComponents(actual));
    }

    private static IPlateSliceBitmapGenerator CreateGenerator(PngSliceExportMethod method) => method switch
    {
        PngSliceExportMethod.MeshIntersection => new MeshIntersectionSliceBitmapGenerator(),
        PngSliceExportMethod.OrthographicProjection => new OrthographicProjectionSliceBitmapGenerator(),
        _ => throw new ArgumentOutOfRangeException(nameof(method), method, null),
    };

    private static SliceBitmap BuildExpectedBitmap(Func<float, float, bool> containsPoint)
    {
        var bitmap = new SliceBitmap(PixelWidth, PixelHeight);
        for (var row = 0; row < PixelHeight; row++)
        {
            var z = RowToZ(row);
            for (var column = 0; column < PixelWidth; column++)
            {
                var x = ColumnToX(column);
                if (containsPoint(x, z))
                    bitmap.SetPixel(column, row, 255);
            }
        }

        return bitmap;
    }

    private static void AssertBitmapsMatch(SliceBitmap expected, SliceBitmap actual, float minIou, float maxAreaErrorRatio)
    {
        Assert.Equal(expected.Width, actual.Width);
        Assert.Equal(expected.Height, actual.Height);

        var expectedLit = expected.CountLitPixels();
        var actualLit = actual.CountLitPixels();
        Assert.True(expectedLit > 0);

        var intersection = 0;
        var union = 0;
        for (var i = 0; i < expected.Pixels.Length; i++)
        {
            var expectedOn = expected.Pixels[i] > 0;
            var actualOn = actual.Pixels[i] > 0;

            if (expectedOn && actualOn)
                intersection++;
            if (expectedOn || actualOn)
                union++;
        }

        var areaErrorRatio = Math.Abs(actualLit - expectedLit) / (float)expectedLit;
        var iou = union == 0 ? 1f : intersection / (float)union;

        Assert.True(iou >= minIou, $"Expected IoU >= {minIou}, actual {iou}.");
        Assert.True(areaErrorRatio <= maxAreaErrorRatio, $"Expected area error <= {maxAreaErrorRatio}, actual {areaErrorRatio}.");
    }

    private static float ColumnToX(int column)
        => (((column + 0.5f) / PixelWidth) * BedWidthMm) - (BedWidthMm * 0.5f);

    private static float RowToZ(int row)
        => (BedDepthMm * 0.5f) - (((row + 0.5f) / PixelHeight) * BedDepthMm);

    private static List<Triangle3D> CreateCube(float sideMm)
        => CreateOffsetCube(sideMm, centerX: 0f, centerZ: 0f);

    private static List<Triangle3D> CreateOffsetCube(float sideMm, float centerX, float centerZ)
        => CreateOffsetBox(sideMm, sideMm, sideMm, centerX, centerZ);

    private static List<Triangle3D> CreateOffsetBox(float widthMm, float depthMm, float heightMm, float centerX, float centerZ)
    {
        var halfWidth = widthMm * 0.5f;
        var halfDepth = depthMm * 0.5f;
        var p000 = new Vec3(centerX - halfWidth, 0f, centerZ - halfDepth);
        var p001 = new Vec3(centerX - halfWidth, 0f, centerZ + halfDepth);
        var p010 = new Vec3(centerX - halfWidth, heightMm, centerZ - halfDepth);
        var p011 = new Vec3(centerX - halfWidth, heightMm, centerZ + halfDepth);
        var p100 = new Vec3(centerX + halfWidth, 0f, centerZ - halfDepth);
        var p101 = new Vec3(centerX + halfWidth, 0f, centerZ + halfDepth);
        var p110 = new Vec3(centerX + halfWidth, heightMm, centerZ - halfDepth);
        var p111 = new Vec3(centerX + halfWidth, heightMm, centerZ + halfDepth);

        return
        [
            MakeTriangle(p000, p001, p101), MakeTriangle(p000, p101, p100),
            MakeTriangle(p010, p110, p111), MakeTriangle(p010, p111, p011),
            MakeTriangle(p000, p100, p110), MakeTriangle(p000, p110, p010),
            MakeTriangle(p001, p011, p111), MakeTriangle(p001, p111, p101),
            MakeTriangle(p000, p010, p011), MakeTriangle(p000, p011, p001),
            MakeTriangle(p100, p101, p111), MakeTriangle(p100, p111, p110),
        ];
    }

    private static List<Triangle3D> CreateCylinder(float radiusMm, float heightMm, int segments)
    {
        var triangles = new List<Triangle3D>(segments * 4);
        var bottomCenter = new Vec3(0f, 0f, 0f);
        var topCenter = new Vec3(0f, heightMm, 0f);

        for (var i = 0; i < segments; i++)
        {
            var a0 = (MathF.PI * 2f * i) / segments;
            var a1 = (MathF.PI * 2f * (i + 1)) / segments;
            var b0 = new Vec3(radiusMm * MathF.Cos(a0), 0f, radiusMm * MathF.Sin(a0));
            var b1 = new Vec3(radiusMm * MathF.Cos(a1), 0f, radiusMm * MathF.Sin(a1));
            var t0 = new Vec3(b0.X, heightMm, b0.Z);
            var t1 = new Vec3(b1.X, heightMm, b1.Z);

            triangles.Add(MakeTriangle(b0, b1, t1));
            triangles.Add(MakeTriangle(b0, t1, t0));
            triangles.Add(MakeTriangle(bottomCenter, b1, b0));
            triangles.Add(MakeTriangle(topCenter, t0, t1));
        }

        return triangles;
    }

    private static List<Triangle3D> CreateSphere(float radiusMm, int latSegments, int lonSegments)
    {
        var triangles = new List<Triangle3D>(latSegments * lonSegments * 2);
        var center = new Vec3(0f, radiusMm, 0f);

        for (var lat = 0; lat < latSegments; lat++)
        {
            var theta0 = MathF.PI * lat / latSegments;
            var theta1 = MathF.PI * (lat + 1) / latSegments;

            for (var lon = 0; lon < lonSegments; lon++)
            {
                var phi0 = (MathF.PI * 2f * lon) / lonSegments;
                var phi1 = (MathF.PI * 2f * (lon + 1)) / lonSegments;

                var p00 = SpherePoint(center, radiusMm, theta0, phi0);
                var p01 = SpherePoint(center, radiusMm, theta0, phi1);
                var p10 = SpherePoint(center, radiusMm, theta1, phi0);
                var p11 = SpherePoint(center, radiusMm, theta1, phi1);

                if (lat == 0)
                {
                    triangles.Add(MakeTriangle(p00, p11, p10));
                }
                else if (lat == latSegments - 1)
                {
                    triangles.Add(MakeTriangle(p00, p01, p11));
                }
                else
                {
                    triangles.Add(MakeTriangle(p00, p01, p11));
                    triangles.Add(MakeTriangle(p00, p11, p10));
                }
            }
        }

        return triangles;
    }

    private static List<Triangle3D> CreateTriangularPyramid(float baseRadiusMm, float heightMm)
    {
        var (a, b, c) = GetBaseTriangle(baseRadiusMm);
        var apex = new Vec3(0f, heightMm, 0f);

        return
        [
            MakeTriangle(a, c, b),
            MakeTriangle(apex, a, b),
            MakeTriangle(apex, b, c),
            MakeTriangle(apex, c, a),
        ];
    }

    private static (Vec3 A, Vec3 B, Vec3 C) GetBaseTriangle(float radiusMm)
    {
        var a = new Vec3(0f, 0f, radiusMm);
        var b = new Vec3(-radiusMm * 0.8660254f, 0f, -radiusMm * 0.5f);
        var c = new Vec3(radiusMm * 0.8660254f, 0f, -radiusMm * 0.5f);
        return (a, b, c);
    }

    private static Vec3 SpherePoint(Vec3 center, float radiusMm, float theta, float phi)
    {
        var sinTheta = MathF.Sin(theta);
        return new Vec3(
            center.X + (radiusMm * sinTheta * MathF.Cos(phi)),
            center.Y + (radiusMm * MathF.Cos(theta)),
            center.Z + (radiusMm * sinTheta * MathF.Sin(phi)));
    }

    private static Triangle3D MakeTriangle(Vec3 a, Vec3 b, Vec3 c)
    {
        var normal = (b - a).Cross(c - a).Normalized;
        return new Triangle3D(a, b, c, normal);
    }

    private static bool PointInTriangle(
        (float X, float Z) p,
        (float X, float Z) a,
        (float X, float Z) b,
        (float X, float Z) c)
    {
        var d1 = Sign(p, a, b);
        var d2 = Sign(p, b, c);
        var d3 = Sign(p, c, a);
        var hasNegative = d1 < 0 || d2 < 0 || d3 < 0;
        var hasPositive = d1 > 0 || d2 > 0 || d3 > 0;
        return !(hasNegative && hasPositive);
    }

    private static List<int> CountLitRuns(ReadOnlySpan<byte> span)
    {
        var runs = new List<int>();
        var index = 0;

        while (index < span.Length)
        {
            while (index < span.Length && span[index] == 0)
                index++;

            var start = index;
            while (index < span.Length && span[index] > 0)
                index++;

            if (start < index)
                runs.Add(index - start);
        }

        return runs;
    }

    private static int CountConnectedComponents(SliceBitmap bitmap)
    {
        var visited = new bool[bitmap.Pixels.Length];
        var components = 0;

        for (var row = 0; row < bitmap.Height; row++)
        {
            for (var column = 0; column < bitmap.Width; column++)
            {
                var index = (row * bitmap.Width) + column;
                if (visited[index] || bitmap.Pixels[index] == 0)
                    continue;

                components++;
                var queue = new Queue<(int X, int Y)>();
                visited[index] = true;
                queue.Enqueue((column, row));

                while (queue.Count > 0)
                {
                    var current = queue.Dequeue();
                    foreach (var (nextX, nextY) in new[]
                             {
                                 (current.X - 1, current.Y),
                                 (current.X + 1, current.Y),
                                 (current.X, current.Y - 1),
                                 (current.X, current.Y + 1),
                             })
                    {
                        if (nextX < 0 || nextY < 0 || nextX >= bitmap.Width || nextY >= bitmap.Height)
                            continue;

                        var nextIndex = (nextY * bitmap.Width) + nextX;
                        if (visited[nextIndex] || bitmap.Pixels[nextIndex] == 0)
                            continue;

                        visited[nextIndex] = true;
                        queue.Enqueue((nextX, nextY));
                    }
                }
            }
        }

        return components;
    }

    private static bool PointInAxisAlignedSquare(float x, float z, float centerX, float centerZ, float half)
        => MathF.Abs(x - centerX) <= half && MathF.Abs(z - centerZ) <= half;

    private static float Sign((float X, float Z) p1, (float X, float Z) p2, (float X, float Z) p3)
        => ((p1.X - p3.X) * (p2.Z - p3.Z)) - ((p2.X - p3.X) * (p1.Z - p3.Z));
}
