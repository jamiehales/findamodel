using System.Collections.Concurrent;
using System.Diagnostics;
using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;

const float BedWidthMm = 218.88f;
const float BedDepthMm = 122.88f;
const float LayerHeightMm = 0.05f;

Console.WriteLine("=== Slice Component Profiler ===");
Console.WriteLine($"CPU: {Environment.ProcessorCount} logical processors");
Console.WriteLine();

var allTriangles = BuildRealisticPlateGeometry();
Console.WriteLine($"Geometry: {allTriangles.Count:N0} triangles");

var cpuOrtho = new OrthographicProjectionSliceBitmapGenerator();
var meshIntersection = new MeshIntersectionSliceBitmapGenerator();

// Warmup
cpuOrtho.RenderLayerBitmap(allTriangles, 1.0f, BedWidthMm, BedDepthMm, 960, 600, LayerHeightMm);
meshIntersection.RenderLayerBitmap(allTriangles, 1.0f, BedWidthMm, BedDepthMm, 960, 600, LayerHeightMm);
GC.Collect(2, GCCollectionMode.Forced, true, true);

var sliceHeights = new[] { 0.025f, 0.5f, 1.0f, 2.0f, 5.0f, 10.0f };
var resolutions = new[] { (960, 600, "960x600"), (3840, 2400, "3840x2400") };

Console.WriteLine("\n--- Single Layer: Ortho vs MeshIntersection ---");
Console.WriteLine($"{"Height",-8} {"Res",-12} {"Ortho ms",10} {"Mesh ms",10} {"Ortho px",10} {"Mesh px",10}");
foreach (var height in sliceHeights)
{
    foreach (var (w, h, label) in resolutions)
    {
        var sw1 = Stopwatch.StartNew();
        var b1 = cpuOrtho.RenderLayerBitmap(allTriangles, height, BedWidthMm, BedDepthMm, w, h, LayerHeightMm);
        sw1.Stop();
        var sw2 = Stopwatch.StartNew();
        var b2 = meshIntersection.RenderLayerBitmap(allTriangles, height, BedWidthMm, BedDepthMm, w, h, LayerHeightMm);
        sw2.Stop();
        Console.WriteLine($"{height,-8:F3} {label,-12} {sw1.Elapsed.TotalMilliseconds,10:F2} {sw2.Elapsed.TotalMilliseconds,10:F2} {b1.CountLitPixels(),10} {b2.CountLitPixels(),10}");
    }
}

Console.WriteLine("\n--- CPU Parallel Utilization (Ortho) ---");
var proc = Process.GetCurrentProcess();
foreach (var (w, h, label) in resolutions)
{
    proc.Refresh();
    var cpuBefore = proc.TotalProcessorTime;
    var wallSw = Stopwatch.StartNew();
    cpuOrtho.RenderLayerBitmap(allTriangles, 2.0f, BedWidthMm, BedDepthMm, w, h, LayerHeightMm);
    wallSw.Stop();
    proc.Refresh();
    var cpuAfter = proc.TotalProcessorTime;
    var cpuMs = (cpuAfter - cpuBefore).TotalMilliseconds;
    Console.WriteLine($"  {label}: wall={wallSw.Elapsed.TotalMilliseconds:F1}ms cpu={cpuMs:F1}ms parallelism={cpuMs / wallSw.Elapsed.TotalMilliseconds:F2}x");
}

// Component breakdown at 3840x2400
Console.WriteLine("\n--- Component Breakdown (3840x2400, height=2.0mm) ---");
{
    const int pw = 3840, ph = 2400;
    const float sliceH = 2.0f;

    var sw = Stopwatch.StartNew();
    var precomp = OrthographicProjectionSliceBitmapGenerator.BuildPrecomputedTriangles(allTriangles);
    sw.Stop();
    Console.WriteLine($"  BuildPrecomputedTriangles: {sw.Elapsed.TotalMilliseconds:F2}ms ({allTriangles.Count} tris)");

    sw.Restart();
    var candidates = OrthographicProjectionSliceBitmapGenerator.BuildRowCandidates(allTriangles, sliceH, LayerHeightMm, BedDepthMm, ph);
    sw.Stop();
    var activeCount = candidates.Count(c => c is { Count: > 0 });
    var totalCandidates = candidates.Where(c => c != null).Sum(c => c!.Count);
    Console.WriteLine($"  BuildRowCandidates: {sw.Elapsed.TotalMilliseconds:F2}ms (active rows: {activeCount}/{ph}, total entries: {totalCandidates})");

    // Measure FillProjectedRow serial
    var bitmap = new SliceBitmap(pw, ph);
    sw.Restart();
    for (var row = 0; row < ph; row++)
    {
        var cands = candidates[row];
        if (cands == null || cands.Count == 0) continue;
        var zMm = OrthographicProjectionSliceBitmapGenerator.RowToZ(row, BedDepthMm, ph);
        OrthographicProjectionSliceBitmapGenerator.FillProjectedRow(bitmap.GetRowSpan(row), precomp, cands, sliceH, zMm, BedWidthMm, pw);
    }
    sw.Stop();
    Console.WriteLine($"  FillProjectedRow (serial, all rows): {sw.Elapsed.TotalMilliseconds:F2}ms");

    // Measure FillProjectedRow parallel (ForEach Partitioner)
    bitmap = new SliceBitmap(pw, ph);
    var activeRows = new List<int>(activeCount);
    for (var row = 0; row < ph; row++)
        if (candidates[row] is { Count: > 0 }) activeRows.Add(row);

    sw.Restart();
    Parallel.ForEach(
        Partitioner.Create(0, activeRows.Count),
        new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount },
        range =>
        {
            for (var i = range.Item1; i < range.Item2; i++)
            {
                var row = activeRows[i];
                var cands = candidates[row]!;
                var zMm = OrthographicProjectionSliceBitmapGenerator.RowToZ(row, BedDepthMm, ph);
                OrthographicProjectionSliceBitmapGenerator.FillProjectedRow(bitmap.GetRowSpan(row), precomp, cands, sliceH, zMm, BedWidthMm, pw);
            }
        });
    sw.Stop();
    Console.WriteLine($"  FillProjectedRow (Parallel.ForEach partitioned): {sw.Elapsed.TotalMilliseconds:F2}ms");

    // Measure FillProjectedRow parallel (Parallel.For active rows)
    bitmap = new SliceBitmap(pw, ph);
    sw.Restart();
    Parallel.For(0, activeRows.Count, new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount }, i =>
    {
        var row = activeRows[i];
        var cands = candidates[row]!;
        var zMm = OrthographicProjectionSliceBitmapGenerator.RowToZ(row, BedDepthMm, ph);
        OrthographicProjectionSliceBitmapGenerator.FillProjectedRow(bitmap.GetRowSpan(row), precomp, cands, sliceH, zMm, BedWidthMm, pw);
    });
    sw.Stop();
    Console.WriteLine($"  FillProjectedRow (Parallel.For active rows): {sw.Elapsed.TotalMilliseconds:F2}ms");

    // Cleanup cost - broken down by sub-step
    // First render a fresh bitmap for cleanup timing
    var cleanupBitmap = new SliceBitmap(pw, ph);
    {
        var cb_candidates = OrthographicProjectionSliceBitmapGenerator.BuildRowCandidates(allTriangles, sliceH, LayerHeightMm, BedDepthMm, ph);
        var cb_precomp = OrthographicProjectionSliceBitmapGenerator.BuildPrecomputedTriangles(allTriangles);
        for (var row = 0; row < ph; row++)
        {
            var cands = cb_candidates[row];
            if (cands == null || cands.Count == 0) continue;
            var zMm = OrthographicProjectionSliceBitmapGenerator.RowToZ(row, BedDepthMm, ph);
            OrthographicProjectionSliceBitmapGenerator.FillProjectedRow(cleanupBitmap.GetRowSpan(row), cb_precomp, cands, sliceH, zMm, BedWidthMm, pw);
        }
    }
    Console.WriteLine($"  Lit pixels before cleanup: {cleanupBitmap.CountLitPixels():N0}");

    // Clone original for the initial pass
    var originalClone = (byte[])cleanupBitmap.Pixels.Clone();

    sw.Restart();
    // Initial support check (the first loop in RemoveUnsupportedHorizontalPixels)
    for (var y = 0; y < ph; y++)
    {
        var x = 0;
        while (x < pw)
        {
            var index = (y * pw) + x;
            if (originalClone[index] == 0) { x++; continue; }
            var runStart = x;
            while (x + 1 < pw && originalClone[(y * pw) + x + 1] > 0) x++;
            x++; // just iterate, don't modify
        }
    }
    sw.Stop();
    Console.WriteLine($"  Initial support check loop: {sw.Elapsed.TotalMilliseconds:F2}ms");

    // Now measure each cleanup sub-step on a fresh copy
    var cleanupBitmap2 = new SliceBitmap(pw, ph);
    Array.Copy(cleanupBitmap.Pixels, cleanupBitmap2.Pixels, cleanupBitmap.Pixels.Length);

    sw.Restart();
    cleanupBitmap2.ClearUnsupportedRunInteriors(originalClone);
    sw.Stop();
    Console.WriteLine($"  ClearUnsupportedRunInteriors: {sw.Elapsed.TotalMilliseconds:F2}ms");

    sw.Restart();
    cleanupBitmap2.RepairVerticalDropouts(2);
    sw.Stop();
    Console.WriteLine($"  RepairVerticalDropouts(2): {sw.Elapsed.TotalMilliseconds:F2}ms");

    sw.Restart();
    cleanupBitmap2.FillSmallInteriorVoids();
    sw.Stop();
    Console.WriteLine($"  FillSmallInteriorVoids: {sw.Elapsed.TotalMilliseconds:F2}ms");

    sw.Restart();
    cleanupBitmap2.RepairThinInteriorHorizontalGaps();
    sw.Stop();
    Console.WriteLine($"  RepairThinInteriorHorizontalGaps: {sw.Elapsed.TotalMilliseconds:F2}ms");

    sw.Restart();
    cleanupBitmap2.RemoveDetachedArtifacts();
    sw.Stop();
    Console.WriteLine($"  RemoveDetachedArtifacts: {sw.Elapsed.TotalMilliseconds:F2}ms");

    sw.Restart();
    cleanupBitmap.RemoveUnsupportedHorizontalPixels();
    sw.Stop();
    Console.WriteLine($"  Full RemoveUnsupportedHorizontalPixels: {sw.Elapsed.TotalMilliseconds:F2}ms");

    // Full path for reference
    sw.Restart();
    cpuOrtho.RenderLayerBitmap(allTriangles, sliceH, BedWidthMm, BedDepthMm, pw, ph, LayerHeightMm);
    sw.Stop();
    Console.WriteLine($"  Full RenderLayerBitmap: {sw.Elapsed.TotalMilliseconds:F2}ms");
}

Console.WriteLine("\n--- Batch vs Single (Ortho, 960x600) ---");
foreach (var bs in new[] { 1, 2, 4, 8, 16, 32 })
{
    var heights = Enumerable.Range(0, bs).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
    var trisByLayer = heights.Select(_ => (IReadOnlyList<Triangle3D>)allTriangles).ToArray();
    proc.Refresh();
    var cpuBefore = proc.TotalProcessorTime;
    var sw = Stopwatch.StartNew();
    cpuOrtho.RenderLayerBitmaps(trisByLayer, heights, BedWidthMm, BedDepthMm, 960, 600, LayerHeightMm);
    sw.Stop();
    proc.Refresh();
    var cpuMs = (proc.TotalProcessorTime - cpuBefore).TotalMilliseconds;
    Console.WriteLine($"  batch={bs}: wall={sw.Elapsed.TotalMilliseconds:F1}ms ({sw.Elapsed.TotalMilliseconds / bs:F1}ms/layer) parallelism={cpuMs / Math.Max(1, sw.Elapsed.TotalMilliseconds):F2}x");
}

Console.WriteLine("\n--- GPU vs CPU ---");
try
{
    using var gpuContext = new GlSliceProjectionContext(NullLoggerFactory.Instance);
    if (gpuContext.IsAvailable)
    {
        Console.WriteLine($"  backend: {gpuContext.ActiveBackend}");
        gpuContext.TryRenderBatch(allTriangles, [1.0f], BedWidthMm, BedDepthMm, 960, 600);
        foreach (var (w, h, label) in resolutions)
        {
            var swCpu = Stopwatch.StartNew();
            cpuOrtho.RenderLayerBitmap(allTriangles, 2.0f, BedWidthMm, BedDepthMm, w, h, LayerHeightMm);
            swCpu.Stop();
            var swGpu = Stopwatch.StartNew();
            gpuContext.TryRenderBatch(allTriangles, [2.0f], BedWidthMm, BedDepthMm, w, h);
            swGpu.Stop();
            Console.WriteLine($"  {label}: cpu={swCpu.Elapsed.TotalMilliseconds:F1}ms gpu={swGpu.Elapsed.TotalMilliseconds:F1}ms speedup={swCpu.Elapsed.TotalMilliseconds / swGpu.Elapsed.TotalMilliseconds:F1}x");
        }
        foreach (var bs in new[] { 1, 4, 16, 32 })
        {
            var heights = Enumerable.Range(0, bs).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
            var sw = Stopwatch.StartNew();
            gpuContext.TryRenderBatch(allTriangles, heights, BedWidthMm, BedDepthMm, 3840, 2400);
            sw.Stop();
            Console.WriteLine($"  GPU batch={bs} 3840x2400 (raw): {sw.Elapsed.TotalMilliseconds:F1}ms ({sw.Elapsed.TotalMilliseconds / bs:F1}ms/layer)");
        }

        // GPU batch with sequential cleanup (old approach)
        foreach (var bs in new[] { 1, 16, 32 })
        {
            var heights = Enumerable.Range(0, bs).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
            var sw = Stopwatch.StartNew();
            var bitmaps = gpuContext.TryRenderBatch(allTriangles, heights, BedWidthMm, BedDepthMm, 3840, 2400);
            if (bitmaps != null) foreach (var b in bitmaps) b.RemoveUnsupportedHorizontalPixels();
            sw.Stop();
            Console.WriteLine($"  GPU batch={bs} 3840x2400 (seq cleanup): {sw.Elapsed.TotalMilliseconds:F1}ms ({sw.Elapsed.TotalMilliseconds / bs:F1}ms/layer)");
        }

        // GPU batch with overlapped parallel cleanup (new approach)
        foreach (var bs in new[] { 1, 16, 32 })
        {
            var heights = Enumerable.Range(0, bs).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
            var cleanupTasks = new ConcurrentBag<Task>();
            var sw = Stopwatch.StartNew();
            var bitmaps = gpuContext.TryRenderBatch(allTriangles, heights, BedWidthMm, BedDepthMm, 3840, 2400,
                onBitmapReady: b => cleanupTasks.Add(Task.Run(() => b.RemoveUnsupportedHorizontalPixels())));
            Task.WaitAll(cleanupTasks.ToArray());
            sw.Stop();
            Console.WriteLine($"  GPU batch={bs} 3840x2400 (overlap cleanup): {sw.Elapsed.TotalMilliseconds:F1}ms ({sw.Elapsed.TotalMilliseconds / bs:F1}ms/layer)");
        }
    }
    else Console.WriteLine("  GPU unavailable");
}
catch (Exception ex) { Console.WriteLine($"  GPU error: {ex.Message}"); }

Console.WriteLine("\n--- Memory (Ortho, 32 batch, 3840x2400) ---");
GC.Collect(2, GCCollectionMode.Forced, true, true);
var baseMem = GC.GetTotalMemory(true);
var g0 = GC.CollectionCount(0); var g1 = GC.CollectionCount(1); var g2 = GC.CollectionCount(2);
var bh = Enumerable.Range(0, 32).Select(i => 0.025f + (i * LayerHeightMm)).ToArray();
var bt = bh.Select(_ => (IReadOnlyList<Triangle3D>)allTriangles).ToArray();
var results = cpuOrtho.RenderLayerBitmaps(bt, bh, BedWidthMm, BedDepthMm, 3840, 2400, LayerHeightMm);
var peakMem = GC.GetTotalMemory(false);
Console.WriteLine($"  base={baseMem / 1024.0 / 1024:F1}MB peak={peakMem / 1024.0 / 1024:F1}MB delta={(peakMem - baseMem) / 1024.0 / 1024:F1}MB");
Console.WriteLine($"  GC gen0={GC.CollectionCount(0) - g0} gen1={GC.CollectionCount(1) - g1} gen2={GC.CollectionCount(2) - g2}");
Console.WriteLine($"  Bitmap total: {results.Count * 3840L * 2400 / 1024.0 / 1024:F1}MB");

Console.WriteLine("\n=== Complete ===");

static List<Triangle3D> BuildRealisticPlateGeometry()
{
    const float bedWidthMm = 218.88f; const float bedDepthMm = 122.88f;
    var placements = new[] {
        (X: 104.0f, Y: 123.1f, A: -0.0002f, T: "complex"), (X: 113.1f, Y: 99.6f, A: 0.841f, T: "complex"),
        (X: 139.1f, Y: 106.5f, A: 0.535f, T: "small"), (X: 29.0f, Y: 120.3f, A: -1.602f, T: "small"),
        (X: 156.4f, Y: 120.2f, A: 1.612f, T: "small"), (X: 213.9f, Y: 119.4f, A: 3.135f, T: "small"),
        (X: 47.0f, Y: 118.9f, A: -0.015f, T: "small"), (X: 146.5f, Y: 88.8f, A: 2.425f, T: "small"),
        (X: 62.3f, Y: 108.9f, A: -0.417f, T: "small"), (X: 9.2f, Y: 118.7f, A: -3.746f, T: "small"),
        (X: 130.1f, Y: 75.7f, A: -0.365f, T: "small"), (X: 18.1f, Y: 103.7f, A: 2.194f, T: "small"),
        (X: 159.7f, Y: 102.4f, A: -0.722f, T: "small"), (X: 176.0f, Y: 118.8f, A: -3.813f, T: "small"),
        (X: 184.4f, Y: 103.4f, A: 2.084f, T: "small"), (X: 80.6f, Y: 106.9f, A: 0.181f, T: "small"),
        (X: 158.7f, Y: 76.2f, A: 2.606f, T: "small"), (X: 196.9f, Y: 118.8f, A: 3.249f, T: "small"),
        (X: 172.3f, Y: 89.1f, A: 0.806f, T: "small"), (X: 89.6f, Y: 89.7f, A: 0.362f, T: "small"),
        (X: 96.7f, Y: 48.3f, A: 0.044f, T: "medium"),
    };
    var all = new List<Triangle3D>();
    foreach (var p in placements)
    {
        var cx = p.X - (bedWidthMm * 0.5f); var cz = -(p.Y - (bedDepthMm * 0.5f));
        var (r, h, lat, lon) = p.T switch { "complex" => (7.5f, 20f, 32, 64), "medium" => (6f, 15f, 24, 48), _ => (4f, 12f, 16, 32) };
        all.AddRange(CreateSphere(r, h, lat, lon, cx, cz, p.A));
    }
    return all;
}

static List<Triangle3D> CreateSphere(float r, float h, int latS, int lonS, float ox, float oz, float angle)
{
    var tris = new List<Triangle3D>(latS * lonS * 2);
    var cosA = MathF.Cos(angle); var sinA = MathF.Sin(angle);
    Vec3 Xf(Vec3 v) => new((v.X * cosA) - (v.Z * sinA) + ox, v.Y, (v.X * sinA) + (v.Z * cosA) + oz);
    for (var lat = 0; lat < latS; lat++)
    {
        var t0 = MathF.PI * lat / latS; var t1 = MathF.PI * (lat + 1) / latS;
        for (var lon = 0; lon < lonS; lon++)
        {
            var p0 = MathF.PI * 2f * lon / lonS; var p1 = MathF.PI * 2f * (lon + 1) / lonS;
            var a = Xf(new Vec3(r * MathF.Sin(t0) * MathF.Cos(p0), h * 0.5f + h * 0.5f * MathF.Cos(t0), r * MathF.Sin(t0) * MathF.Sin(p0)));
            var b = Xf(new Vec3(r * MathF.Sin(t0) * MathF.Cos(p1), h * 0.5f + h * 0.5f * MathF.Cos(t0), r * MathF.Sin(t0) * MathF.Sin(p1)));
            var c = Xf(new Vec3(r * MathF.Sin(t1) * MathF.Cos(p0), h * 0.5f + h * 0.5f * MathF.Cos(t1), r * MathF.Sin(t1) * MathF.Sin(p0)));
            var d = Xf(new Vec3(r * MathF.Sin(t1) * MathF.Cos(p1), h * 0.5f + h * 0.5f * MathF.Cos(t1), r * MathF.Sin(t1) * MathF.Sin(p1)));
            if (lat == 0) { var n = (d - a).Cross(c - a).Normalized; tris.Add(new Triangle3D(a, d, c, n)); }
            else if (lat == latS - 1) { var n = (b - a).Cross(d - a).Normalized; tris.Add(new Triangle3D(a, b, d, n)); }
            else { tris.Add(new Triangle3D(a, b, d, (b - a).Cross(d - a).Normalized)); tris.Add(new Triangle3D(a, d, c, (d - a).Cross(c - a).Normalized)); }
        }
    }
    return tris;
}
