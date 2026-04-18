using System.Diagnostics;
using findamodel.Services;
using Xunit;
using Xunit.Abstractions;

namespace findamodel.Tests;

public class SliceBenchmarkTests(ITestOutputHelper output)
{
    [Fact]
    public void Benchmark_CpuOrthographicSlice_VariousComplexities()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_SLICE_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        var generator = new OrthographicProjectionSliceBitmapGenerator();

        var configs = new (string Label, List<Triangle3D> Triangles, float BedW, float BedD, int PxW, int PxH)[]
        {
            ("small-cube-180x180", CreateCube(10f), 30f, 30f, 180, 180),
            ("grid-9408tris-480x480", CreateGrid(14, 14, 4f, 4f, 3f), 80f, 80f, 480, 480),
            ("grid-9408tris-960x600", CreateGrid(14, 14, 4f, 4f, 3f), 80f, 50f, 960, 600),
            ("dense-grid-80000tris-960x600", CreateGrid(40, 40, 2f, 2f, 1.5f), 120f, 120f, 960, 600),
            ("dense-grid-80000tris-3840x2400", CreateGrid(40, 40, 2f, 2f, 1.5f), 120f, 120f, 3840, 2400),
            ("sphere-4096tris-960x600", CreateSphere(12f, 32, 64), 30f, 30f, 960, 600),
        };

        const int warmup = 1;
        const int iterations = 5;

        foreach (var (label, triangles, bedW, bedD, pxW, pxH) in configs)
        {
            for (var w = 0; w < warmup; w++)
                generator.RenderLayerBitmap(triangles, 1.5f, bedW, bedD, pxW, pxH);

            var sw = new Stopwatch();
            var samples = new double[iterations];
            for (var i = 0; i < iterations; i++)
            {
                sw.Restart();
                generator.RenderLayerBitmap(triangles, 1.5f, bedW, bedD, pxW, pxH);
                sw.Stop();
                samples[i] = sw.Elapsed.TotalMilliseconds;
            }

            var avg = samples.Average();
            var min = samples.Min();
            var max = samples.Max();
            output.WriteLine($"{label}: triangles={triangles.Count} avg={avg:F2}ms min={min:F2}ms max={max:F2}ms");
        }
    }

    [Fact]
    public void Benchmark_CpuOrthographicBatchSlice()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_SLICE_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        var generator = new OrthographicProjectionSliceBitmapGenerator();
        var triangles = CreateGrid(14, 14, 4f, 4f, 3f);

        const int layerCount = 16;
        var trianglesByLayer = new IReadOnlyList<Triangle3D>[layerCount];
        var sliceHeights = new float[layerCount];
        for (var i = 0; i < layerCount; i++)
        {
            trianglesByLayer[i] = triangles;
            sliceHeights[i] = 0.05f * (i + 1);
        }

        // warmup
        generator.RenderLayerBitmaps(trianglesByLayer, sliceHeights, 80f, 80f, 960, 600);

        const int iterations = 5;
        var samples = new double[iterations];
        var sw = new Stopwatch();
        for (var i = 0; i < iterations; i++)
        {
            sw.Restart();
            generator.RenderLayerBitmaps(trianglesByLayer, sliceHeights, 80f, 80f, 960, 600);
            sw.Stop();
            samples[i] = sw.Elapsed.TotalMilliseconds;
        }

        var avg = samples.Average();
        output.WriteLine($"batch-16layers-9408tris-960x600: avg={avg:F2}ms min={samples.Min():F2}ms max={samples.Max():F2}ms");
    }

    private static List<Triangle3D> CreateCube(float sideMm)
    {
        var half = sideMm * 0.5f;
        var p000 = new Vec3(-half, 0f, -half);
        var p001 = new Vec3(-half, 0f, half);
        var p010 = new Vec3(-half, sideMm, -half);
        var p011 = new Vec3(-half, sideMm, half);
        var p100 = new Vec3(half, 0f, -half);
        var p101 = new Vec3(half, 0f, half);
        var p110 = new Vec3(half, sideMm, -half);
        var p111 = new Vec3(half, sideMm, half);

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

    private static List<Triangle3D> CreateGrid(int countX, int countZ, float spacingX, float spacingZ, float boxSize)
    {
        var triangles = new List<Triangle3D>(countX * countZ * 12);
        var halfBoxSize = boxSize * 0.5f;
        var offsetX = -(countX * spacingX) * 0.5f;
        var offsetZ = -(countZ * spacingZ) * 0.5f;

        for (var ix = 0; ix < countX; ix++)
        {
            for (var iz = 0; iz < countZ; iz++)
            {
                var cx = offsetX + (ix * spacingX) + (spacingX * 0.5f);
                var cz = offsetZ + (iz * spacingZ) + (spacingZ * 0.5f);

                var p000 = new Vec3(cx - halfBoxSize, 0f, cz - halfBoxSize);
                var p001 = new Vec3(cx - halfBoxSize, 0f, cz + halfBoxSize);
                var p010 = new Vec3(cx - halfBoxSize, boxSize, cz - halfBoxSize);
                var p011 = new Vec3(cx - halfBoxSize, boxSize, cz + halfBoxSize);
                var p100 = new Vec3(cx + halfBoxSize, 0f, cz - halfBoxSize);
                var p101 = new Vec3(cx + halfBoxSize, 0f, cz + halfBoxSize);
                var p110 = new Vec3(cx + halfBoxSize, boxSize, cz - halfBoxSize);
                var p111 = new Vec3(cx + halfBoxSize, boxSize, cz + halfBoxSize);

                triangles.Add(MakeTriangle(p000, p001, p101));
                triangles.Add(MakeTriangle(p000, p101, p100));
                triangles.Add(MakeTriangle(p010, p110, p111));
                triangles.Add(MakeTriangle(p010, p111, p011));
                triangles.Add(MakeTriangle(p000, p100, p110));
                triangles.Add(MakeTriangle(p000, p110, p010));
                triangles.Add(MakeTriangle(p001, p011, p111));
                triangles.Add(MakeTriangle(p001, p111, p101));
                triangles.Add(MakeTriangle(p000, p010, p011));
                triangles.Add(MakeTriangle(p000, p011, p001));
                triangles.Add(MakeTriangle(p100, p101, p111));
                triangles.Add(MakeTriangle(p100, p111, p110));
            }
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
                    triangles.Add(MakeTriangle(p00, p11, p10));
                else if (lat == latSegments - 1)
                    triangles.Add(MakeTriangle(p00, p01, p11));
                else
                {
                    triangles.Add(MakeTriangle(p00, p01, p11));
                    triangles.Add(MakeTriangle(p00, p11, p10));
                }
            }
        }

        return triangles;
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
}
