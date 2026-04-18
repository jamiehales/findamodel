using System.Diagnostics;
using System.Runtime;
using System.Text.Json;
using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;
using Xunit.Abstractions;

namespace findamodel.Tests;

public class SliceProfilingBenchmarkTests(ITestOutputHelper output)
{
    // Realistic printer config: typical MSLA resin printer
    private const float BedWidthMm = 218.88f;
    private const float BedDepthMm = 122.88f;
    private const int PixelWidth = 3840;
    private const int PixelHeight = 2400;
    private const float LayerHeightMm = 0.05f;

    // Lower res for faster iteration
    private const int LowResWidth = 960;
    private const int LowResHeight = 600;

    [Fact]
    public void Profile_CpuOrthographic_RealisticPlate()
    {
        var (allTriangles, groups) = BuildRealisticPlateGeometry();
        output.WriteLine($"Realistic plate: {groups.Count} groups, total triangles={allTriangles.Count:N0}");
        foreach (var (label, tris) in groups)
            output.WriteLine($"  {label}: {tris.Count:N0} triangles");

        var generator = new OrthographicProjectionSliceBitmapGenerator();

        // Pick layers at different heights to profile various scenarios
        var sliceHeights = new[] { 0.025f, 0.5f, 1.0f, 2.0f, 5.0f, 10.0f };

        // Warmup
        generator.RenderLayerBitmap(allTriangles, 1.0f, BedWidthMm, BedDepthMm, LowResWidth, LowResHeight, LayerHeightMm);
        GC.Collect(2, GCCollectionMode.Forced, true, true);

        output.WriteLine("\n=== CPU Orthographic Projection (single layer) ===");
        output.WriteLine($"{"Height mm",-12} {"Res",-12} {"Time ms",10} {"Lit px",10} {"Tris/sec",14}");

        foreach (var height in sliceHeights)
        {
            foreach (var (resW, resH, resLabel) in new[] { (LowResWidth, LowResHeight, "960x600"), (PixelWidth, PixelHeight, "3840x2400") })
            {
                var sw = Stopwatch.StartNew();
                var bitmap = generator.RenderLayerBitmap(allTriangles, height, BedWidthMm, BedDepthMm, resW, resH, LayerHeightMm);
                sw.Stop();
                var trisPerSec = allTriangles.Count / sw.Elapsed.TotalSeconds;
                output.WriteLine($"{height,-12:F3} {resLabel,-12} {sw.Elapsed.TotalMilliseconds,10:F2} {bitmap.CountLitPixels(),10} {trisPerSec,14:N0}");
            }
        }

        // Batch rendering (16 layers)
        output.WriteLine("\n=== CPU Orthographic Projection (batch 16 layers) ===");
        var batchHeights = Enumerable.Range(0, 16).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
        var batchTriangles = batchHeights.Select(_ => (IReadOnlyList<Triangle3D>)allTriangles).ToArray();

        foreach (var (resW, resH, resLabel) in new[] { (LowResWidth, LowResHeight, "960x600"), (PixelWidth, PixelHeight, "3840x2400") })
        {
            GC.Collect(2, GCCollectionMode.Forced, true, true);
            var sw = Stopwatch.StartNew();
            var bitmaps = generator.RenderLayerBitmaps(batchTriangles, batchHeights, BedWidthMm, BedDepthMm, resW, resH, LayerHeightMm);
            sw.Stop();
            var totalLit = bitmaps.Sum(b => b.CountLitPixels());
            output.WriteLine($"batch-16 {resLabel,-12} {sw.Elapsed.TotalMilliseconds,10:F2} {totalLit,10} {sw.Elapsed.TotalMilliseconds / 16,10:F2} ms/layer");
        }
    }

    [Fact]
    public void Profile_CpuMeshIntersection_RealisticPlate()
    {
        var (allTriangles, groups) = BuildRealisticPlateGeometry();
        output.WriteLine($"Realistic plate: {groups.Count} groups, total triangles={allTriangles.Count:N0}");

        var generator = new MeshIntersectionSliceBitmapGenerator();

        // Warmup
        generator.RenderLayerBitmap(allTriangles, 1.0f, BedWidthMm, BedDepthMm, LowResWidth, LowResHeight, LayerHeightMm);
        GC.Collect(2, GCCollectionMode.Forced, true, true);

        var sliceHeights = new[] { 0.025f, 0.5f, 1.0f, 2.0f, 5.0f, 10.0f };

        output.WriteLine("\n=== CPU Mesh Intersection (single layer) ===");
        output.WriteLine($"{"Height mm",-12} {"Res",-12} {"Time ms",10} {"Lit px",10} {"Tris/sec",14}");

        foreach (var height in sliceHeights)
        {
            foreach (var (resW, resH, resLabel) in new[] { (LowResWidth, LowResHeight, "960x600"), (PixelWidth, PixelHeight, "3840x2400") })
            {
                var sw = Stopwatch.StartNew();
                var bitmap = generator.RenderLayerBitmap(allTriangles, height, BedWidthMm, BedDepthMm, resW, resH, LayerHeightMm);
                sw.Stop();
                var trisPerSec = allTriangles.Count / sw.Elapsed.TotalSeconds;
                output.WriteLine($"{height,-12:F3} {resLabel,-12} {sw.Elapsed.TotalMilliseconds,10:F2} {bitmap.CountLitPixels(),10} {trisPerSec,14:N0}");
            }
        }
    }

    [Fact]
    public void Profile_GpuOrthographic_RealisticPlate()
    {
        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        if (!gpuContext.IsAvailable)
        {
            output.WriteLine("GPU context unavailable - skipping");
            return;
        }

        var (allTriangles, groups) = BuildRealisticPlateGeometry();
        output.WriteLine($"Realistic plate: {groups.Count} groups, total triangles={allTriangles.Count:N0}");
        output.WriteLine($"GPU backend: {gpuContext.ActiveBackend}");

        var sliceHeights = new[] { 0.025f, 0.5f, 1.0f, 2.0f, 5.0f, 10.0f };

        // Warmup
        gpuContext.TryRenderBatch(allTriangles, [1.0f], BedWidthMm, BedDepthMm, LowResWidth, LowResHeight);
        GC.Collect(2, GCCollectionMode.Forced, true, true);

        output.WriteLine("\n=== GPU Orthographic Projection (single layer) ===");
        output.WriteLine($"{"Height mm",-12} {"Res",-12} {"Time ms",10} {"Lit px",10}");

        foreach (var height in sliceHeights)
        {
            foreach (var (resW, resH, resLabel) in new[] { (LowResWidth, LowResHeight, "960x600"), (PixelWidth, PixelHeight, "3840x2400") })
            {
                var sw = Stopwatch.StartNew();
                var bitmaps = gpuContext.TryRenderBatch(allTriangles, [height], BedWidthMm, BedDepthMm, resW, resH);
                sw.Stop();
                var litPixels = bitmaps is { Count: > 0 } ? bitmaps[0].CountLitPixels() : 0;
                output.WriteLine($"{height,-12:F3} {resLabel,-12} {sw.Elapsed.TotalMilliseconds,10:F2} {litPixels,10}");
            }
        }

        // Batch rendering
        output.WriteLine("\n=== GPU Orthographic Projection (batch 16 layers) ===");
        var batchHeights = Enumerable.Range(0, 16).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();

        foreach (var (resW, resH, resLabel) in new[] { (LowResWidth, LowResHeight, "960x600"), (PixelWidth, PixelHeight, "3840x2400") })
        {
            GC.Collect(2, GCCollectionMode.Forced, true, true);
            var sw = Stopwatch.StartNew();
            var bitmaps = gpuContext.TryRenderBatch(allTriangles, batchHeights, BedWidthMm, BedDepthMm, resW, resH);
            sw.Stop();
            var totalLit = bitmaps?.Sum(b => b.CountLitPixels()) ?? 0;
            output.WriteLine($"batch-16 {resLabel,-12} {sw.Elapsed.TotalMilliseconds,10:F2} {totalLit,10} {sw.Elapsed.TotalMilliseconds / 16,10:F2} ms/layer");
        }
    }

    [Fact]
    public void Profile_FullArchiveGeneration_RealisticPlate()
    {
        var (allTriangles, _) = BuildRealisticPlateGeometry();
        output.WriteLine($"Total triangles: {allTriangles.Count:N0}");

        using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
        var orthographic = gpuContext.IsAvailable
            ? new OrthographicProjectionSliceBitmapGenerator(gpuContext, NullLoggerFactory.Instance)
            : new OrthographicProjectionSliceBitmapGenerator();

        var rasterService = new PlateSliceRasterService([
            new MeshIntersectionSliceBitmapGenerator(),
            orthographic,
        ]);

        // Archive generation for first 2mm (40 layers at 0.05mm)
        var archiveLayerHeight = 0.05f;

        foreach (var method in new[] { PngSliceExportMethod.MeshIntersection, PngSliceExportMethod.OrthographicProjection })
        {
            GC.Collect(2, GCCollectionMode.Forced, true, true);
            var memBefore = GC.GetTotalMemory(false);

            var sw = Stopwatch.StartNew();
            var zip = rasterService.GenerateSliceArchive(
                allTriangles,
                bedWidthMm: BedWidthMm,
                bedDepthMm: BedDepthMm,
                resolutionX: LowResWidth,
                resolutionY: LowResHeight,
                method: method,
                layerHeightMm: archiveLayerHeight);
            sw.Stop();

            var memAfter = GC.GetTotalMemory(false);
            var maxY = allTriangles.Max(t => MathF.Max(t.V0.Y, MathF.Max(t.V1.Y, t.V2.Y)));
            var layerCount = (int)Math.Ceiling(maxY / archiveLayerHeight);

            output.WriteLine($"\n=== Archive {method} 960x600 ===");
            output.WriteLine($"  layers={layerCount} elapsed={sw.Elapsed.TotalSeconds:F2}s zipBytes={zip.Length:N0}");
            output.WriteLine($"  ms/layer={sw.Elapsed.TotalMilliseconds / layerCount:F2}");
            output.WriteLine($"  memDelta={((memAfter - memBefore) / 1024.0 / 1024):F1} MB");
        }
    }

    [Fact]
    public void Profile_SliceBitmapCleanup_Overhead()
    {
        var (allTriangles, _) = BuildRealisticPlateGeometry();
        var generator = new OrthographicProjectionSliceBitmapGenerator();

        // Render without cleanup to measure raw render time
        output.WriteLine("=== SliceBitmap Cleanup Overhead ===");

        var sliceHeights = new[] { 0.5f, 2.0f, 5.0f };

        foreach (var height in sliceHeights)
        {
            // Full render (includes cleanup)
            var swFull = Stopwatch.StartNew();
            var fullBitmap = generator.RenderLayerBitmap(allTriangles, height, BedWidthMm, BedDepthMm, LowResWidth, LowResHeight, LayerHeightMm);
            swFull.Stop();

            output.WriteLine($"height={height:F1} fullRender={swFull.Elapsed.TotalMilliseconds:F2}ms litPixels={fullBitmap.CountLitPixels()}");
        }
    }

    [Fact]
    public void Profile_MemoryAndGc_During_BatchSlicing()
    {
        var (allTriangles, _) = BuildRealisticPlateGeometry();
        var generator = new OrthographicProjectionSliceBitmapGenerator();
        output.WriteLine($"Total triangles: {allTriangles.Count:N0}");

        GC.Collect(2, GCCollectionMode.Forced, true, true);
        var baseMemory = GC.GetTotalMemory(true);
        var gen0Before = GC.CollectionCount(0);
        var gen1Before = GC.CollectionCount(1);
        var gen2Before = GC.CollectionCount(2);

        var batchHeights = Enumerable.Range(0, 32).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
        var batchTriangles = batchHeights.Select(_ => (IReadOnlyList<Triangle3D>)allTriangles).ToArray();

        var sw = Stopwatch.StartNew();
        var bitmaps = generator.RenderLayerBitmaps(batchTriangles, batchHeights, BedWidthMm, BedDepthMm, LowResWidth, LowResHeight, LayerHeightMm);
        sw.Stop();

        var peakMemory = GC.GetTotalMemory(false);
        var gen0After = GC.CollectionCount(0);
        var gen1After = GC.CollectionCount(1);
        var gen2After = GC.CollectionCount(2);

        output.WriteLine($"\n=== Memory & GC Profile (32 layers batch, 960x600) ===");
        output.WriteLine($"  elapsed={sw.Elapsed.TotalMilliseconds:F2}ms ({sw.Elapsed.TotalMilliseconds / 32:F2} ms/layer)");
        output.WriteLine($"  baseMemory={baseMemory / 1024.0 / 1024:F1} MB, peakMemory={peakMemory / 1024.0 / 1024:F1} MB");
        output.WriteLine($"  memDelta={(peakMemory - baseMemory) / 1024.0 / 1024:F1} MB");
        output.WriteLine($"  GC gen0={gen0After - gen0Before}, gen1={gen1After - gen1Before}, gen2={gen2After - gen2Before}");
        output.WriteLine($"  bitmapSize={LowResWidth * LowResHeight} bytes ({LowResWidth * LowResHeight / 1024.0:F0} KB) x {bitmaps.Count} = {LowResWidth * LowResHeight * bitmaps.Count / 1024.0 / 1024:F1} MB total");
    }

    [Fact]
    public void Profile_ComponentBreakdown_CpuOrthographic()
    {
        var (allTriangles, _) = BuildRealisticPlateGeometry();
        output.WriteLine($"Total triangles: {allTriangles.Count:N0}");

        // Detailed component timing using reflection or direct calls
        var sliceHeight = 2.0f;

        // Time precomputation
        var swPrecompute = Stopwatch.StartNew();
        // BuildPrecomputedTriangles is private - we'll time full render and decompose
        swPrecompute.Stop();

        // Time full render at various resolutions to see scaling
        output.WriteLine("\n=== Resolution Scaling Profile ===");
        output.WriteLine($"{"Resolution",-16} {"Time ms",10} {"Lit px",10} {"Px/ms",10} {"Bandwidth MB/s",14}");

        var generator = new OrthographicProjectionSliceBitmapGenerator();
        var resolutions = new[] { (240, 150), (480, 300), (960, 600), (1920, 1200), (3840, 2400) };

        foreach (var (w, h) in resolutions)
        {
            // Warmup
            generator.RenderLayerBitmap(allTriangles, sliceHeight, BedWidthMm, BedDepthMm, w, h, LayerHeightMm);

            var iterations = 3;
            var totalMs = 0.0;
            var lit = 0;

            for (var i = 0; i < iterations; i++)
            {
                var sw = Stopwatch.StartNew();
                var bitmap = generator.RenderLayerBitmap(allTriangles, sliceHeight, BedWidthMm, BedDepthMm, w, h, LayerHeightMm);
                sw.Stop();
                totalMs += sw.Elapsed.TotalMilliseconds;
                lit = bitmap.CountLitPixels();
            }

            var avgMs = totalMs / iterations;
            var pxPerMs = (w * h) / avgMs;
            var bandwidthMBs = ((double)w * h) / avgMs / 1000;
            output.WriteLine($"{w}x{h,-11} {avgMs,10:F2} {lit,10} {pxPerMs,10:F0} {bandwidthMBs,14:F2}");
        }
    }

    // Builds geometry that mimics the plate-export-repro-payload.json layout:
    // 21 model placements with varying rotations, producing ~100K-500K triangles
    private static (List<Triangle3D> AllTriangles, List<(string Label, List<Triangle3D> Triangles)> Groups) BuildRealisticPlateGeometry()
    {
        // Simulate the 21 placements from plate-export-repro-payload.json
        // Each placement gets a procedural model with realistic complexity
        var placements = new[]
        {
            (X: 104.0f, Y: 123.1f, Angle: -0.0002f, ModelType: "complex"),     // 0896ca56
            (X: 113.1f, Y: 99.6f, Angle: 0.841f, ModelType: "complex"),        // f0a6e388
            (X: 139.1f, Y: 106.5f, Angle: 0.535f, ModelType: "small"),         // 22ffa3d1 inst0
            (X: 29.0f, Y: 120.3f, Angle: -1.602f, ModelType: "small"),         // inst1
            (X: 156.4f, Y: 120.2f, Angle: 1.612f, ModelType: "small"),         // inst2
            (X: 213.9f, Y: 119.4f, Angle: 3.135f, ModelType: "small"),         // inst3
            (X: 47.0f, Y: 118.9f, Angle: -0.015f, ModelType: "small"),         // inst4
            (X: 146.5f, Y: 88.8f, Angle: 2.425f, ModelType: "small"),          // inst5
            (X: 62.3f, Y: 108.9f, Angle: -0.417f, ModelType: "small"),         // inst6
            (X: 9.2f, Y: 118.7f, Angle: -3.746f, ModelType: "small"),          // inst7
            (X: 130.1f, Y: 75.7f, Angle: -0.365f, ModelType: "small"),         // inst8
            (X: 18.1f, Y: 103.7f, Angle: 2.194f, ModelType: "small"),          // inst9
            (X: 159.7f, Y: 102.4f, Angle: -0.722f, ModelType: "small"),        // inst10
            (X: 176.0f, Y: 118.8f, Angle: -3.813f, ModelType: "small"),        // inst11
            (X: 184.4f, Y: 103.4f, Angle: 2.084f, ModelType: "small"),         // inst12
            (X: 80.6f, Y: 106.9f, Angle: 0.181f, ModelType: "small"),          // inst13
            (X: 158.7f, Y: 76.2f, Angle: 2.606f, ModelType: "small"),          // inst14
            (X: 196.9f, Y: 118.8f, Angle: 3.249f, ModelType: "small"),         // inst15
            (X: 172.3f, Y: 89.1f, Angle: 0.806f, ModelType: "small"),          // inst16
            (X: 89.6f, Y: 89.7f, Angle: 0.362f, ModelType: "small"),           // inst17
            (X: 96.7f, Y: 48.3f, Angle: 0.044f, ModelType: "medium"),          // 70a980a5
        };

        var allTriangles = new List<Triangle3D>();
        var groups = new List<(string Label, List<Triangle3D> Triangles)>();

        for (var i = 0; i < placements.Length; i++)
        {
            var p = placements[i];
            // Convert from plate coords (origin top-left) to centered bed coords
            var centerX = p.X - (BedWidthMm * 0.5f);
            var centerZ = -(p.Y - (BedDepthMm * 0.5f));

            List<Triangle3D> modelTriangles;
            switch (p.ModelType)
            {
                case "complex":
                    // Larger model (~15mm diameter, ~20mm tall, ~8K tris - like a detailed figurine)
                    modelTriangles = CreateSphere(7.5f, 20f, 32, 64, centerX, centerZ, p.Angle);
                    break;

                case "medium":
                    // Medium model (~12mm diameter, ~15mm tall, ~4K tris)
                    modelTriangles = CreateSphere(6f, 15f, 24, 48, centerX, centerZ, p.Angle);
                    break;

                case "small":
                default:
                    // Small model (~8mm diameter, ~12mm tall, ~2K tris - like a token/miniature)
                    modelTriangles = CreateSphere(4f, 12f, 16, 32, centerX, centerZ, p.Angle);
                    break;
            }

            groups.Add(($"placement-{i}-{p.ModelType}", modelTriangles));
            allTriangles.AddRange(modelTriangles);
        }

        return (allTriangles, groups);
    }

    private static List<Triangle3D> CreateSphere(
        float radiusMm,
        float heightMm,
        int latSegments,
        int lonSegments,
        float offsetX,
        float offsetZ,
        float angleRad)
    {
        var triangles = new List<Triangle3D>(latSegments * lonSegments * 2);
        var cosA = MathF.Cos(angleRad);
        var sinA = MathF.Sin(angleRad);

        Vec3 Transform(Vec3 v)
        {
            var x = (v.X * cosA) - (v.Z * sinA) + offsetX;
            var z = (v.X * sinA) + (v.Z * cosA) + offsetZ;
            return new Vec3(x, v.Y, z);
        }

        // Create an elongated shape (taller than wide, like a miniature)
        for (var lat = 0; lat < latSegments; lat++)
        {
            var theta0 = MathF.PI * lat / latSegments;
            var theta1 = MathF.PI * (lat + 1) / latSegments;

            for (var lon = 0; lon < lonSegments; lon++)
            {
                var phi0 = MathF.PI * 2f * lon / lonSegments;
                var phi1 = MathF.PI * 2f * (lon + 1) / lonSegments;

                var sinT0 = MathF.Sin(theta0);
                var sinT1 = MathF.Sin(theta1);
                var cosT0 = MathF.Cos(theta0);
                var cosT1 = MathF.Cos(theta1);
                var sinP0 = MathF.Sin(phi0);
                var sinP1 = MathF.Sin(phi1);
                var cosP0 = MathF.Cos(phi0);
                var cosP1 = MathF.Cos(phi1);

                var p00 = Transform(new Vec3(radiusMm * sinT0 * cosP0, (heightMm * 0.5f) + (heightMm * 0.5f * cosT0), radiusMm * sinT0 * sinP0));
                var p01 = Transform(new Vec3(radiusMm * sinT0 * cosP1, (heightMm * 0.5f) + (heightMm * 0.5f * cosT0), radiusMm * sinT0 * sinP1));
                var p10 = Transform(new Vec3(radiusMm * sinT1 * cosP0, (heightMm * 0.5f) + (heightMm * 0.5f * cosT1), radiusMm * sinT1 * sinP0));
                var p11 = Transform(new Vec3(radiusMm * sinT1 * cosP1, (heightMm * 0.5f) + (heightMm * 0.5f * cosT1), radiusMm * sinT1 * sinP1));

                if (lat == 0)
                {
                    var n = (p11 - p00).Cross(p10 - p00).Normalized;
                    triangles.Add(new Triangle3D(p00, p11, p10, n));
                }
                else if (lat == latSegments - 1)
                {
                    var n = (p01 - p00).Cross(p11 - p00).Normalized;
                    triangles.Add(new Triangle3D(p00, p01, p11, n));
                }
                else
                {
                    var n1 = (p01 - p00).Cross(p11 - p00).Normalized;
                    triangles.Add(new Triangle3D(p00, p01, p11, n1));
                    var n2 = (p11 - p00).Cross(p10 - p00).Normalized;
                    triangles.Add(new Triangle3D(p00, p11, p10, n2));
                }
            }
        }

        return triangles;
    }

    private static Triangle3D MakeTriangle(Vec3 a, Vec3 b, Vec3 c)
    {
        var normal = (b - a).Cross(c - a).Normalized;
        return new Triangle3D(a, b, c, normal);
    }
}
