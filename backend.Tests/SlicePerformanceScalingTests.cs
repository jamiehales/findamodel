using System.Collections;
using System.Diagnostics;
using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;
using Xunit.Abstractions;

namespace findamodel.Tests;

public class SlicePerformanceScalingTests(ITestOutputHelper output)
{
    private const float BedWidthMm = 32f;
    private const float BedDepthMm = 32f;
    private const float SliceHeightMm = 5f;
    private const float LayerHeightMm = 0.05f;

    [Theory]
    [InlineData(PngSliceExportMethod.MeshIntersection)]
    [InlineData(PngSliceExportMethod.OrthographicProjection)]
    public void OneMillionPolygonMesh_RendersNonEmptySlice(PngSliceExportMethod method)
    {
        var generator = CreateGenerator(method);
        var triangles = new ProceduralCuboidGridMesh(targetTriangleCount: 1_000_000, footprintWidthMm: 18f, footprintDepthMm: 18f, heightMm: 10f);

        var sw = Stopwatch.StartNew();
        var bitmap = generator.RenderLayerBitmap(
            triangles,
            SliceHeightMm,
            BedWidthMm,
            BedDepthMm,
            pixelWidth: 96,
            pixelHeight: 96,
            layerThicknessMm: LayerHeightMm);
        sw.Stop();

        output.WriteLine($"method={method} triangles={triangles.Count:N0} elapsedMs={sw.Elapsed.TotalMilliseconds:F2} litPixels={bitmap.CountLitPixels()}");

        Assert.True(bitmap.CountLitPixels() > 0, "Expected a non-empty slice bitmap.");
        Assert.True(bitmap.CountLitPixels() < bitmap.Pixels.Length, "Expected the slice to occupy only part of the build area.");
    }

    [Fact]
    public void Benchmark_SyntheticMeshes_UpToHundredMillionPolygons()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_SLICE_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        var cases = new[] { 1_000_000, 5_000_000, 10_000_000, 100_000_000 };
        foreach (var triangleCount in cases)
        {
            var triangles = new ProceduralCuboidGridMesh(
                triangleCount,
                footprintWidthMm: 20f,
                footprintDepthMm: 20f,
                heightMm: 10f);

            foreach (var method in new[] { PngSliceExportMethod.MeshIntersection, PngSliceExportMethod.OrthographicProjection })
            {
                var generator = CreateGenerator(method);
                var sw = Stopwatch.StartNew();
                var bitmap = generator.RenderLayerBitmap(
                    triangles,
                    SliceHeightMm,
                    BedWidthMm,
                    BedDepthMm,
                    pixelWidth: 48,
                    pixelHeight: 48,
                    layerThicknessMm: LayerHeightMm);
                sw.Stop();

                output.WriteLine(
                    $"benchmark method={method} triangles={triangles.Count:N0} elapsedMs={sw.Elapsed.TotalMilliseconds:F2} litPixels={bitmap.CountLitPixels()}");

                Assert.True(bitmap.CountLitPixels() > 0, $"Expected non-empty output for {method} at {triangleCount:N0} triangles.");
            }
        }
    }

    [Fact]
    public void Benchmark_OrthographicProjection_CpuVsGpu_WhenAvailable()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_SLICE_GPU_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        if (!gpuContext.IsAvailable)
        {
            output.WriteLine("slice gpu benchmark skipped: GL context unavailable");
            return;
        }

        var triangles = new ProceduralCuboidGridMesh(1_000_000, footprintWidthMm: 18f, footprintDepthMm: 18f, heightMm: 10f);
        var cpuGenerator = new OrthographicProjectionSliceBitmapGenerator();
        var gpuGenerator = new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance);

        var cpu = Measure(() => cpuGenerator.RenderLayerBitmap(triangles, SliceHeightMm, BedWidthMm, BedDepthMm, 96, 96, LayerHeightMm));
        var gpu = Measure(() => gpuGenerator.RenderLayerBitmap(triangles, SliceHeightMm, BedWidthMm, BedDepthMm, 96, 96, LayerHeightMm));

        output.WriteLine($"slice orthographic backend={gpuContext.ActiveBackend}");
        output.WriteLine($"slice orthographic cpu ms={cpu.ElapsedMs:F2} litPixels={cpu.LitPixels}");
        output.WriteLine($"slice orthographic gpu ms={gpu.ElapsedMs:F2} litPixels={gpu.LitPixels}");

        Assert.True(cpu.LitPixels > 0);
        Assert.True(gpu.LitPixels > 0);
    }

    [Fact]
    public void Benchmark_OrthographicProjection_BatchVsSingle_WhenAvailable()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_SLICE_GPU_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        if (!gpuContext.IsAvailable)
        {
            output.WriteLine("slice gpu batch benchmark skipped: GL context unavailable");
            return;
        }

        var triangles = new ProceduralCuboidGridMesh(1_000_000, footprintWidthMm: 18f, footprintDepthMm: 18f, heightMm: 10f);
        var gpuGenerator = new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance);
        var batchGenerator = Assert.IsAssignableFrom<IBatchPlateSliceBitmapGenerator>(gpuGenerator);
        var sliceHeights = Enumerable.Range(0, 8).Select(i => 2f + (i * 0.5f)).ToArray();
        var trianglesByLayer = sliceHeights.Select(_ => (IReadOnlyList<Triangle3D>)triangles).ToArray();

        var singleSw = Stopwatch.StartNew();
        foreach (var sliceHeight in sliceHeights)
            gpuGenerator.RenderLayerBitmap(triangles, sliceHeight, BedWidthMm, BedDepthMm, 96, 96, LayerHeightMm);
        singleSw.Stop();

        var batchSw = Stopwatch.StartNew();
        var batched = batchGenerator.RenderLayerBitmaps(trianglesByLayer, sliceHeights, BedWidthMm, BedDepthMm, 96, 96, LayerHeightMm);
        batchSw.Stop();

        output.WriteLine($"slice batch backend={gpuContext.ActiveBackend} singleMs={singleSw.Elapsed.TotalMilliseconds:F2} batchMs={batchSw.Elapsed.TotalMilliseconds:F2}");

        Assert.Equal(sliceHeights.Length, batched.Count);
        Assert.All(batched, bitmap => Assert.True(bitmap.CountLitPixels() > 0));
    }

    [Fact]
    public void Benchmark_PngSliceArchive_Throughput()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_SLICE_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        var orthographic = gpuContext.IsAvailable
            ? new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance)
            : new OrthographicProjectionSliceBitmapGenerator();

        var rasterService = new PlateSliceRasterService(
        [
            new MeshIntersectionSliceBitmapGenerator(),
            orthographic,
        ]);

        var triangles = new ProceduralCuboidGridMesh(
            targetTriangleCount: 1_000_000,
            footprintWidthMm: 20f,
            footprintDepthMm: 20f,
            heightMm: 12f);

        foreach (var method in new[] { PngSliceExportMethod.MeshIntersection, PngSliceExportMethod.OrthographicProjection })
        {
            var sw = Stopwatch.StartNew();
            var zip = rasterService.GenerateSliceArchive(
                triangles,
                bedWidthMm: BedWidthMm,
                bedDepthMm: BedDepthMm,
                resolutionX: 96,
                resolutionY: 96,
                method: method,
                layerHeightMm: 0.5f);
            sw.Stop();

            output.WriteLine($"archive backend={gpuContext.ActiveBackend} method={method} elapsedMs={sw.Elapsed.TotalMilliseconds:F2} zipBytes={zip.Length}");
            Assert.True(zip.Length > 0, $"Expected non-empty slice archive for {method}.");
        }
    }

    private static (double ElapsedMs, int LitPixels) Measure(Func<SliceBitmap> render)
    {
        var sw = Stopwatch.StartNew();
        var bitmap = render();
        sw.Stop();
        return (sw.Elapsed.TotalMilliseconds, bitmap.CountLitPixels());
    }

    private static IPlateSliceBitmapGenerator CreateGenerator(PngSliceExportMethod method) => method switch
    {
        PngSliceExportMethod.MeshIntersection => new MeshIntersectionSliceBitmapGenerator(),
        PngSliceExportMethod.OrthographicProjection => new OrthographicProjectionSliceBitmapGenerator(),
        _ => throw new ArgumentOutOfRangeException(nameof(method), method, null),
    };

    private sealed class ProceduralCuboidGridMesh(int targetTriangleCount, float footprintWidthMm, float footprintDepthMm, float heightMm)
        : IReadOnlyList<Triangle3D>
    {
        private const int TrianglesPerCuboid = 12;
        private readonly int cuboidCount = Math.Max(1, targetTriangleCount / TrianglesPerCuboid);
        private readonly int columns = Math.Max(1, (int)Math.Ceiling(Math.Sqrt(Math.Max(1, targetTriangleCount / (double)TrianglesPerCuboid))));

        public int Count => cuboidCount * TrianglesPerCuboid;

        public Triangle3D this[int index]
        {
            get
            {
                if (index < 0 || index >= Count)
                    throw new ArgumentOutOfRangeException(nameof(index));

                var cuboidIndex = index / TrianglesPerCuboid;
                var triangleIndex = index % TrianglesPerCuboid;
                return CreateTriangle(cuboidIndex, triangleIndex);
            }
        }

        public IEnumerator<Triangle3D> GetEnumerator()
        {
            for (var index = 0; index < Count; index++)
                yield return this[index];
        }

        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();

        private Triangle3D CreateTriangle(int cuboidIndex, int triangleIndex)
        {
            var row = cuboidIndex / columns;
            var column = cuboidIndex % columns;
            var rows = Math.Max(1, (int)Math.Ceiling(cuboidCount / (double)columns));

            var cellWidth = footprintWidthMm / columns;
            var cellDepth = footprintDepthMm / rows;
            var minX = -footprintWidthMm * 0.5f + (column * cellWidth);
            var minZ = -footprintDepthMm * 0.5f + (row * cellDepth);
            var maxX = minX + cellWidth;
            var maxZ = minZ + cellDepth;

            var p000 = new Vec3(minX, 0f, minZ);
            var p001 = new Vec3(minX, 0f, maxZ);
            var p010 = new Vec3(minX, heightMm, minZ);
            var p011 = new Vec3(minX, heightMm, maxZ);
            var p100 = new Vec3(maxX, 0f, minZ);
            var p101 = new Vec3(maxX, 0f, maxZ);
            var p110 = new Vec3(maxX, heightMm, minZ);
            var p111 = new Vec3(maxX, heightMm, maxZ);

            return triangleIndex switch
            {
                0 => new Triangle3D(p000, p001, p101, new Vec3(0f, -1f, 0f)),
                1 => new Triangle3D(p000, p101, p100, new Vec3(0f, -1f, 0f)),
                2 => new Triangle3D(p010, p110, p111, new Vec3(0f, 1f, 0f)),
                3 => new Triangle3D(p010, p111, p011, new Vec3(0f, 1f, 0f)),
                4 => new Triangle3D(p000, p100, p110, new Vec3(0f, 0f, -1f)),
                5 => new Triangle3D(p000, p110, p010, new Vec3(0f, 0f, -1f)),
                6 => new Triangle3D(p001, p011, p111, new Vec3(0f, 0f, 1f)),
                7 => new Triangle3D(p001, p111, p101, new Vec3(0f, 0f, 1f)),
                8 => new Triangle3D(p000, p010, p011, new Vec3(-1f, 0f, 0f)),
                9 => new Triangle3D(p000, p011, p001, new Vec3(-1f, 0f, 0f)),
                10 => new Triangle3D(p100, p101, p111, new Vec3(1f, 0f, 0f)),
                11 => new Triangle3D(p100, p111, p110, new Vec3(1f, 0f, 0f)),
                _ => throw new ArgumentOutOfRangeException(nameof(triangleIndex)),
            };
        }
    }
}
