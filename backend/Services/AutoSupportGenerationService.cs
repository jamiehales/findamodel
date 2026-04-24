using Microsoft.Extensions.Logging;
using System.Diagnostics;
using System.Collections.Concurrent;
using System.Threading;
using System.IO;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Formats.Png;

namespace findamodel.Services;

/// <summary>
/// Method 3: Island-tip detection auto-support generation.
/// Slices the model layer by layer, detects 2-D connected islands at every layer,
/// then places/reinforces supports at overhang starts (first appearance from below).
/// Reinforcement considers peel-force load, compressive crush load, and angular
/// stability so wide planes can gain edge supports even when center pull capacity
/// is sufficient.
/// Only support spheres are returned for visualisation - island outlines are omitted.
/// </summary>
public sealed class AutoSupportGenerationService
{
    private const int TrianglesPerVoxelBlock = 12;
    private const int MaxPreviewVoxelTriangles = 2_000_000;
    private const int MaxReinforcementIterationsPerIsland = 32;
    private const int MaxSolverPasses = 8;
    private const int MaxPreviewSupportPoints = 2000;
    private const int MaxTipIslandAddsPerPass = 4;
    private const float DefaultOverhangSensitivity = 0.65f;
    private const float DefaultPeelStartMultiplier = 1.3f;
    private const float DefaultPeelEndMultiplier = 0.9f;
    private const float DefaultHeightBias = 0.3f;
    private const float DefaultBridgeReductionFactor = 0.3f;
    private const float DefaultCantileverMomentMultiplier = 0.4f;
    private const float DefaultCantileverReferenceLengthMm = 8f;
    private const float DefaultLayerBondStrengthPerMm2 = 1.2f;
    private const float DefaultLayerAdhesionSafetyFactor = 1.1f;
    private const bool DefaultSupportInteractionEnabled = true;
    private const float DefaultDrainageDepthForceMultiplier = 0.15f;
    private const bool DefaultAccessibilityEnabled = true;
    private const int DefaultAccessibilityScanRadiusPx = 6;
    private const int DefaultAccessibilityMinOpenDirections = 1;
    private const float DefaultSurfaceQualityWeight = 0.35f;
    private const int DefaultSurfaceQualitySearchRadiusPx = 6;
    private const bool DefaultOrientationCheckEnabled = true;
    private const float DefaultOrientationRiskForceMultiplierMax = 1.35f;
    private const float DefaultOrientationRiskThresholdRatio = 1.15f;
    private static readonly ParallelOptions FullCoreParallelOptions = new()
    {
        MaxDegreeOfParallelism = Math.Max(1, Environment.ProcessorCount),
    };

    private readonly ILogger logger;
    private readonly OrthographicProjectionSliceBitmapGenerator slicer = new();
    private readonly AppConfigService? appConfigService;

    public AutoSupportGenerationService(ILoggerFactory loggerFactory)
        : this(null, loggerFactory)
    {
    }

    public AutoSupportGenerationService(AppConfigService? appConfigService, ILoggerFactory loggerFactory)
    {
        this.appConfigService = appConfigService;
        logger = loggerFactory.CreateLogger<AutoSupportGenerationService>();
    }

    public SupportPreviewResult GenerateSupportPreview(
        LoadedGeometry geometry,
        AutoSupportTuningOverrides? tuningOverrides = null,
        int maxSupportPoints = MaxPreviewSupportPoints)
    {
        ArgumentNullException.ThrowIfNull(geometry);
        if (maxSupportPoints < 1)
            throw new ArgumentOutOfRangeException(nameof(maxSupportPoints), "Support cap must be at least 1.");

        if (geometry.Triangles.Count == 0)
            return new SupportPreviewResult([], CloneGeometry(geometry, []), []);

        var tuning = ResolveTuning(tuningOverrides);
        var bedWidthMm = MathF.Max(geometry.DimensionXMm + (tuning.BedMarginMm * 2f), 10f);
        var bedDepthMm = MathF.Max(geometry.DimensionZMm + (tuning.BedMarginMm * 2f), 10f);
        var voxelSizeMm = Math.Clamp(
            MathF.Max(geometry.DimensionXMm, geometry.DimensionZMm) / 48f,
            tuning.MinVoxelSizeMm,
            tuning.MaxVoxelSizeMm);
        var layerHeightMm = Math.Clamp(geometry.DimensionYMm / 48f, tuning.MinLayerHeightMm, tuning.MaxLayerHeightMm);
        var pixelWidth = Math.Max(24, (int)Math.Ceiling(bedWidthMm / voxelSizeMm));
        var pixelHeight = Math.Max(24, (int)Math.Ceiling(bedDepthMm / voxelSizeMm));
        var orientationRiskMultiplier = tuning.OrientationCheckEnabled
            ? EstimateOrientationRiskMultiplier(geometry, tuning)
            : 1f;

        var layerCount = Math.Max(1, (int)Math.Ceiling(Math.Max(geometry.DimensionYMm, layerHeightMm) / layerHeightMm));

        // Render all layers in one batch so the slicer can parallelize across layers efficiently.
        var layerIslands = new List<TipIsland>[layerCount];
        var layerVoxelRects = new List<VoxelRect>[layerCount];
        var sliceHeightsMm = new float[layerCount];
        var trianglesByLayer = new IReadOnlyList<Triangle3D>[layerCount];
        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            sliceHeightsMm[layerIndex] = (layerIndex * layerHeightMm) + (layerHeightMm * 0.5f);
            trianglesByLayer[layerIndex] = geometry.Triangles;
        }

        var layerBitmaps = slicer.RenderLayerBitmaps(
            trianglesByLayer,
            sliceHeightsMm,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm);

        Parallel.For(0, layerCount, FullCoreParallelOptions, layerIndex =>
        {
            var bitmap = layerBitmaps[layerIndex];
            layerIslands[layerIndex] = FindTipIslands(bitmap, bedWidthMm, bedDepthMm, sliceHeightsMm[layerIndex], tuning.MinIslandAreaMm2);
            layerVoxelRects[layerIndex] = ExtractVoxelRects(bitmap);
        });

        // Detect suction (enclosed regions) per island via edge flood-fill on bitmaps
        if (tuning.SuctionMultiplier > 1f)
        {
            Parallel.For(0, layerCount, FullCoreParallelOptions, layerIndex =>
            {
                var bitmap = layerBitmaps[layerIndex];
                var enclosedPixels = DetectEnclosedPixels(bitmap);
                if (enclosedPixels == null)
                    return;

                for (var islandIndex = 0; islandIndex < layerIslands[layerIndex].Count; islandIndex++)
                {
                    var island = layerIslands[layerIndex][islandIndex];
                    var enclosedCount = 0;
                    foreach (var (x, y) in island.PixelCoords)
                    {
                        if (enclosedPixels.Contains((x, y)))
                            enclosedCount++;
                    }

                    if (enclosedCount > 0)
                    {
                        var ratio = (float)enclosedCount / island.PixelCoords.Count;
                        layerIslands[layerIndex][islandIndex] = island with
                        {
                            HasEnclosedRegion = true,
                            EnclosureRatio = ratio,
                        };
                    }
                }
            });

            var lowestEnclosedLayerByPixel = new Dictionary<(int Column, int Row), int>();
            for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
            {
                var enclosedPixels = DetectEnclosedPixels(layerBitmaps[layerIndex]);
                if (enclosedPixels == null)
                    continue;

                foreach (var pixel in enclosedPixels)
                {
                    if (!lowestEnclosedLayerByPixel.ContainsKey(pixel))
                        lowestEnclosedLayerByPixel[pixel] = layerIndex;
                }
            }

            for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
            {
                for (var islandIndex = 0; islandIndex < layerIslands[layerIndex].Count; islandIndex++)
                {
                    var island = layerIslands[layerIndex][islandIndex];
                    if (!island.HasEnclosedRegion)
                        continue;

                    var minLayer = int.MaxValue;
                    foreach (var pixel in island.PixelCoords)
                    {
                        if (lowestEnclosedLayerByPixel.TryGetValue(pixel, out var firstLayer) && firstLayer < minLayer)
                            minLayer = firstLayer;
                    }

                    if (minLayer == int.MaxValue)
                        continue;

                    var depthLayers = Math.Max(1, layerIndex - minLayer + 1);
                    layerIslands[layerIndex][islandIndex] = island with
                    {
                        EnclosureDepthLayers = depthLayers,
                    };
                }
            }
        }

        // Compute per-layer total pixel area for area-growth detection
        var layerTotalArea = new float[layerCount];
        Parallel.For(0, layerCount, FullCoreParallelOptions, layerIndex =>
        {
            var sum = 0f;
            foreach (var island in layerIslands[layerIndex])
                sum += island.TopLayerAreaMm2;
            layerTotalArea[layerIndex] = sum;
        });

        // Compute per-layer area growth ratio for delta-area force
        var layerAreaGrowthRatio = new float[layerCount];
        Parallel.For(1, layerCount, FullCoreParallelOptions, layerIndex =>
        {
            var prevArea = layerTotalArea[layerIndex - 1];
            if (prevArea > 0.01f)
                layerAreaGrowthRatio[layerIndex] = (layerTotalArea[layerIndex] - prevArea) / prevArea;
        });

        // Build per-layer pixel sets once so each algorithm can re-evaluate support decisions
        // against all currently placed supports from lower layers.
        var layerPixelSets = new HashSet<(int Column, int Row)>?[layerCount];
        var lowestLayerByPixel = new Dictionary<(int Column, int Row), int>();
        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            var islands = layerIslands[layerIndex];
            if (islands.Count == 0)
                continue;

            var set = new HashSet<(int Column, int Row)>();
            foreach (var island in islands)
                foreach (var pixel in island.PixelCoords)
                {
                    set.Add(pixel);
                    if (!lowestLayerByPixel.ContainsKey(pixel))
                        lowestLayerByPixel[pixel] = layerIndex;
                }
            layerPixelSets[layerIndex] = set;
        }

        // An island is an overhang "tip" when none of its pixels exist in the layer below -
        // i.e. it is the first (lowest) appearance of that connected region, requiring a support.
        var prevLayerPixels = new HashSet<(int Column, int Row)>?[layerCount];
        for (var layerIndex = 1; layerIndex < layerCount; layerIndex++)
            prevLayerPixels[layerIndex] = layerPixelSets[layerIndex - 1];

        var supportPoints = new List<SupportPoint>();
        var supportCapReached = false;

        for (var solverPass = 0; solverPass < MaxSolverPasses; solverPass++)
        {
            var changed = false;
            var passStopwatch = Stopwatch.StartNew();
            var supportCountAtPassStart = supportPoints.Count;

            var reinforceStart = Stopwatch.GetTimestamp();
            var supportCountBeforeReinforce = supportPoints.Count;

            changed |= ReinforceSupportsByForceAndSpacing(
                layerIslands,
                layerPixelSets,
                lowestLayerByPixel,
                prevLayerPixels,
                supportPoints,
                maxSupportPoints,
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight,
                layerHeightMm,
                layerAreaGrowthRatio,
                tuning,
                orientationRiskMultiplier);

            var reinforceElapsedMs = Stopwatch.GetElapsedTime(reinforceStart).TotalMilliseconds;
            var reinforceAdded = supportPoints.Count - supportCountBeforeReinforce;

            var shrinkageElapsedMs = 0d;
            var shrinkageAdded = 0;

            if (tuning.ShrinkagePercent > 0f)
            {
                var shrinkageStart = Stopwatch.GetTimestamp();
                var supportCountBeforeShrinkage = supportPoints.Count;
                changed |= ApplyShrinkageEdgeSupports(
                    layerIslands,
                    layerPixelSets,
                    lowestLayerByPixel,
                    prevLayerPixels,
                    supportPoints,
                    maxSupportPoints,
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    layerHeightMm,
                    tuning);
                shrinkageElapsedMs = Stopwatch.GetElapsedTime(shrinkageStart).TotalMilliseconds;
                shrinkageAdded = supportPoints.Count - supportCountBeforeShrinkage;
            }

            var gravityElapsedMs = 0d;
            var upgradedByGravity = 0;
            if (tuning.GravityEnabled)
            {
                var gravityStart = Stopwatch.GetTimestamp();
                changed |= ApplyGravityLoading(
                    layerIslands,
                    prevLayerPixels,
                    supportPoints,
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    layerHeightMm,
                    tuning,
                    out upgradedByGravity);
                gravityElapsedMs = Stopwatch.GetElapsedTime(gravityStart).TotalMilliseconds;
            }

            passStopwatch.Stop();
            logger.LogDebug(
                "V3 solver pass {Pass}/{MaxPasses}: changed={Changed}, supports +{SupportDelta} ({SupportsStart}->{SupportsEnd}), reinforce={ReinforceMs:F1}ms (+{ReinforceAdded}), shrinkage={ShrinkageMs:F1}ms (+{ShrinkageAdded}), gravity={GravityMs:F1}ms (upgraded={GravityUpgrades}), total={PassMs:F1}ms",
                solverPass + 1,
                MaxSolverPasses,
                changed,
                supportPoints.Count - supportCountAtPassStart,
                supportCountAtPassStart,
                supportPoints.Count,
                reinforceElapsedMs,
                reinforceAdded,
                shrinkageElapsedMs,
                shrinkageAdded,
                gravityElapsedMs,
                upgradedByGravity,
                passStopwatch.Elapsed.TotalMilliseconds);

            if (supportPoints.Count >= maxSupportPoints)
            {
                supportCapReached = true;
                break;
            }

            if (!changed)
                break;
        }

        // Keep generated supports at their selected placement layer. Do not
        // run post-pass vertical redistribution that can promote supports to
        // higher layers.

        if (supportCapReached)
        {
            logger.LogWarning(
                "V3 preview support cap reached at {SupportCap} supports; truncating additional placement",
                maxSupportPoints);
        }

        PopulateSupportForces(
            layerIslands,
            prevLayerPixels,
            supportPoints,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm,
            tuning,
            orientationRiskMultiplier);

        var adhesionAdjusted = ApplyLayerAdhesionReinforcement(
            layerPixelSets,
            supportPoints,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm,
            tuning);

        if (adhesionAdjusted)
        {
            PopulateSupportForces(
                layerIslands,
                prevLayerPixels,
                supportPoints,
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight,
                layerHeightMm,
                tuning,
                orientationRiskMultiplier);
        }

        var sliceLayers = BuildSliceLayers(
            layerIslands,
            layerBitmaps,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm);
        var flattenedIslands = sliceLayers.SelectMany(layer => layer.Islands).ToArray();

        var supportTriangles = BuildSupportSphereMesh(supportPoints);

        var voxelTriangles = BuildMergedVoxelMesh(
            layerVoxelRects,
            bedWidthMm,
            bedDepthMm,
            layerHeightMm,
            pixelWidth,
            pixelHeight,
            MaxPreviewVoxelTriangles,
            out var voxelMeshTruncated);

        if (voxelMeshTruncated)
        {
            logger.LogWarning(
                "V3: Truncated merged voxel preview mesh to {KeptTriangles} triangles for stability",
                voxelTriangles.Count);
        }

        var voxelGeometry = CloneGeometry(geometry, voxelTriangles);

        logger.LogInformation(
            "V3: Generated {SupportCount} island-tip support markers for model footprint {X:F1}x{Z:F1} mm ({VoxelTriangles} voxel triangles)",
            supportPoints.Count,
            geometry.DimensionXMm,
            geometry.DimensionZMm,
            voxelTriangles.Count);

        return new SupportPreviewResult(
            supportPoints,
            CloneGeometry(geometry, supportTriangles),
            flattenedIslands,
            sliceLayers,
            voxelGeometry);
    }

    private static List<TipIsland> FindTipIslands(
        SliceBitmap bitmap,
        float bedWidthMm,
        float bedDepthMm,
        float sliceHeightMm,
        float minIslandAreaMm2)
    {
        var visited = new bool[bitmap.Pixels.Length];
        var islands = new List<TipIsland>();
        var pixelWidthMm = bedWidthMm / bitmap.Width;
        var pixelDepthMm = bedDepthMm / bitmap.Height;
        var pixelAreaMm2 = pixelWidthMm * pixelDepthMm;

        for (var row = 0; row < bitmap.Height; row++)
        {
            for (var column = 0; column < bitmap.Width; column++)
            {
                var index = (row * bitmap.Width) + column;
                if (visited[index] || bitmap.Pixels[index] == 0)
                    continue;

                var queue = new Queue<(int X, int Y)>();
                var pixelCoords = new List<(int X, int Y)>();
                var sumX = 0f;
                var sumZ = 0f;
                var minCol = int.MaxValue;
                var maxCol = int.MinValue;
                var minRow = int.MaxValue;
                var maxRow = int.MinValue;
                var perimeterPixelCount = 0;

                visited[index] = true;
                queue.Enqueue((column, row));

                while (queue.Count > 0)
                {
                    var (cx, cy) = queue.Dequeue();
                    pixelCoords.Add((cx, cy));
                    sumX += ColumnToX(cx, bedWidthMm, bitmap.Width);
                    sumZ += RowToZ(cy, bedDepthMm, bitmap.Height);
                    if (cx < minCol) minCol = cx;
                    if (cx > maxCol) maxCol = cx;
                    if (cy < minRow) minRow = cy;
                    if (cy > maxRow) maxRow = cy;

                    var hasEmptyNeighbor = false;
                    foreach (var (nx, ny) in Neighbors(cx, cy))
                    {
                        if (nx < 0 || ny < 0 || nx >= bitmap.Width || ny >= bitmap.Height)
                        {
                            hasEmptyNeighbor = true;
                            continue;
                        }
                        var ni = (ny * bitmap.Width) + nx;
                        if (bitmap.Pixels[ni] == 0)
                        {
                            hasEmptyNeighbor = true;
                            continue;
                        }
                        if (visited[ni])
                            continue;
                        visited[ni] = true;
                        queue.Enqueue((nx, ny));
                    }

                    if (hasEmptyNeighbor)
                        perimeterPixelCount++;
                }

                var areaMm2 = pixelCoords.Count * pixelAreaMm2;
                if (areaMm2 < minIslandAreaMm2)
                    continue;

                var bboxWidthMm = (maxCol - minCol + 1) * pixelWidthMm;
                var bboxHeightMm = (maxRow - minRow + 1) * pixelDepthMm;
                var longSide = MathF.Max(bboxWidthMm, bboxHeightMm);
                var shortSide = MathF.Min(bboxWidthMm, bboxHeightMm);
                var aspectRatio = shortSide > 0.01f ? longSide / shortSide : 1f;
                var perimeterMm = perimeterPixelCount * MathF.Min(pixelWidthMm, pixelDepthMm);
                var perimeterToAreaRatio = areaMm2 > 0.01f ? perimeterMm / areaMm2 : 0f;

                islands.Add(new TipIsland(
                    pixelCoords,
                    sumX / pixelCoords.Count,
                    sumZ / pixelCoords.Count,
                    sliceHeightMm,
                    areaMm2,
                    AspectRatio: aspectRatio,
                    MinWidthMm: shortSide,
                    PerimeterToAreaRatio: perimeterToAreaRatio));
            }
        }

        return islands;
    }

    private static IReadOnlyList<SliceLayerPreview> BuildSliceLayers(
        IReadOnlyList<TipIsland>[] layerIslands,
        IReadOnlyList<SliceBitmap> layerBitmaps,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm)
    {
        var result = new List<SliceLayerPreview>(layerIslands.Length);
        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            var islands = layerIslands[layerIndex];
            var islandPreviews = islands
                .Select(island =>
                {
                    var boundary = ExtractIslandBoundaryMm(
                        island.PixelCoords,
                        island.CentroidX,
                        island.CentroidZ,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight);
                    return new IslandPreview(
                        island.CentroidX,
                        island.CentroidZ,
                        island.SliceHeightMm,
                        island.TopLayerAreaMm2,
                        MathF.Sqrt(island.TopLayerAreaMm2 / MathF.PI),
                        boundary);
                })
                .ToArray();

            var sliceHeight = islandPreviews.Length > 0
                ? islandPreviews[0].SliceHeightMm
                : (layerIndex * layerHeightMm) + (layerHeightMm * 0.5f);
            var sliceMaskPngBase64 = layerIndex < layerBitmaps.Count
                ? EncodeSliceMaskPngBase64(layerBitmaps[layerIndex])
                : null;
            result.Add(new SliceLayerPreview(
                layerIndex,
                sliceHeight,
                islandPreviews,
                bedWidthMm,
                bedDepthMm,
                sliceMaskPngBase64));
        }

        return result;
    }

    private static string EncodeSliceMaskPngBase64(SliceBitmap bitmap)
    {
        using var image = new Image<Rgba32>(bitmap.Width, bitmap.Height);
        for (var y = 0; y < bitmap.Height; y++)
        {
            for (var x = 0; x < bitmap.Width; x++)
            {
                var index = (y * bitmap.Width) + x;
                image[x, y] = bitmap.Pixels[index] == 0
                    ? new Rgba32(0, 0, 0, 0)
                    : new Rgba32(34, 211, 238, 255);
            }
        }

        using var stream = new MemoryStream();
        image.Save(stream, new PngEncoder());
        return Convert.ToBase64String(stream.ToArray());
    }

    private static IReadOnlyList<(float X, float Z)> ExtractIslandBoundaryMm(
        IReadOnlyList<(int X, int Y)> pixels,
        float centroidX,
        float centroidZ,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        if (pixels.Count == 0)
            return [];

        var pixelSet = pixels.ToHashSet();
        var boundaryPoints = new List<(float X, float Z)>();
        foreach (var (x, y) in pixels)
        {
            if (!IsBoundaryPixel(pixelSet, x, y))
                continue;

            boundaryPoints.Add((
                ColumnToX(x, bedWidthMm, pixelWidth),
                RowToZ(y, bedDepthMm, pixelHeight)));
        }

        if (boundaryPoints.Count == 0)
            return [];

        // Keep vertex counts bounded while preserving shape for viewport overlays.
        const int maxBoundaryPoints = 128;
        var ordered = boundaryPoints
            .OrderBy(point => MathF.Atan2(point.Z - centroidZ, point.X - centroidX))
            .ToList();

        if (ordered.Count <= maxBoundaryPoints)
            return ordered;

        var stride = (int)MathF.Ceiling(ordered.Count / (float)maxBoundaryPoints);
        var reduced = new List<(float X, float Z)>();
        for (var i = 0; i < ordered.Count; i += stride)
            reduced.Add(ordered[i]);

        return reduced;
    }

    private static IEnumerable<(int X, int Y)> Neighbors(int x, int y)
    {
        yield return (x - 1, y);
        yield return (x + 1, y);
        yield return (x, y - 1);
        yield return (x, y + 1);
    }

    private static float ColumnToX(int column, float bedWidthMm, int width)
        => (((column + 0.5f) / width) * bedWidthMm) - (bedWidthMm * 0.5f);

    private static float RowToZ(int row, float bedDepthMm, int height)
        => (bedDepthMm * 0.5f) - (((row + 0.5f) / height) * bedDepthMm);

    private static bool ReinforceSupportsByForceAndSpacing(
        List<TipIsland>[] layerIslands,
        HashSet<(int Column, int Row)>?[] layerPixelSets,
        IReadOnlyDictionary<(int Column, int Row), int> lowestLayerByPixel,
        HashSet<(int Column, int Row)>?[] prevLayerPixels,
        List<SupportPoint> supportPoints,
        int maxSupportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        float[] layerAreaGrowthRatio,
        AutoSupportTuning tuning,
        float orientationRiskMultiplier)
    {
        var changed = false;

        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            if (supportPoints.Count >= maxSupportPoints)
                return changed;

            var prevPixels = prevLayerPixels[layerIndex];
            foreach (var island in layerIslands[layerIndex])
            {
                if (supportPoints.Count >= maxSupportPoints)
                    return changed;

                var islandPixelSet = island.PixelCoords.ToHashSet();
                var isTipIsland = IsTipIsland(islandPixelSet, prevPixels);

                var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                if (unsupportedPixels.Count == 0)
                    continue;

                var layerProgress = layerIslands.Length <= 1
                    ? 1f
                    : layerIndex / (float)(layerIslands.Length - 1);

                var localSupportMinY = island.SliceHeightMm - MathF.Max(tuning.SupportSpacingThresholdMm, layerHeightMm);
                var additionsThisIsland = 0;

                for (var i = 0; i < MaxReinforcementIterationsPerIsland; i++)
                {
                    if (isTipIsland && additionsThisIsland >= MaxTipIslandAddsPerPass)
                        break;

                    var supportIndices = FindSupportingSupportsForPixelSet(
                        islandPixelSet,
                        island.SliceHeightMm,
                        supportPoints,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm);

                    if (localSupportMinY > float.NegativeInfinity)
                    {
                        supportIndices = FindSupportingSupportsForPixelSet(
                            islandPixelSet,
                            island.SliceHeightMm,
                            supportPoints,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            layerHeightMm,
                            minimumSupportY: localSupportMinY);
                    }

                    if (supportIndices.Count == 0)
                    {
                        var target = supportPoints.Count == 0
                            ? new FurthestPointCandidate(
                                ComputePixelCentroidMm(unsupportedPixels, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight).XMm,
                                ComputePixelCentroidMm(unsupportedPixels, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight).ZMm,
                                0f)
                            : FindFurthestPixelFromSupports(
                                unsupportedPixels,
                                Enumerable.Range(0, supportPoints.Count).ToList(),
                                supportPoints,
                                bedWidthMm,
                                bedDepthMm,
                                pixelWidth,
                                pixelHeight);

                        var addResult = TryAddSupportAtBestLayer(
                            supportPoints,
                            maxSupportPoints,
                            unsupportedPixels,
                            layerPixelSets,
                            lowestLayerByPixel,
                            layerIndex,
                            target.XMm,
                            target.ZMm,
                            SupportSize.Light,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            layerHeightMm,
                            tuning.SupportSpacingThresholdMm,
                            tuning);
                        if (addResult == SupportPlacementResult.CapReached)
                            return changed;
                        if (addResult == SupportPlacementResult.NoValidPlacement)
                            break;
                        changed = true;
                        additionsThisIsland++;
                        if (!isTipIsland)
                            break;
                        continue;
                    }

                    var furthest = FindFurthestPixelFromSupports(
                        unsupportedPixels,
                        supportIndices,
                        supportPoints,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight);

                    var supportForces = EvaluateSupportForces(
                        unsupportedPixels,
                        supportIndices,
                        supportPoints,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        tuning,
                        island,
                        layerAreaGrowthRatio[layerIndex],
                        prevPixels,
                        layerProgress,
                        orientationRiskMultiplier);

                    var interaction = AnalyzeSupportInteraction(
                        supportIndices,
                        supportPoints,
                        tuning.SupportSpacingThresholdMm,
                        tuning.SupportInteractionEnabled);
                    var combinedCapacity = supportIndices.Sum(index => ComputeCapacity(supportPoints[index], tuning))
                        * interaction.CapacityMultiplier;
                    var verticalPullForce = supportForces.Sum(metric => metric.VerticalPullForce)
                        * interaction.LoadMultiplier;
                    var maxCompressiveForce = supportForces.Max(metric => metric.CompressiveForce);
                    var maxAngularForce = supportForces.Max(metric => metric.AngularForce);
                    var isOverloaded = verticalPullForce > combinedCapacity;
                    var exceedsCrushForce = maxCompressiveForce > tuning.CrushForceThreshold;
                    var exceedsAngularForce = maxAngularForce > tuning.MaxAngularForce;
                    var exceedsSpacing = furthest.DistanceMm > tuning.SupportSpacingThresholdMm;
                    var exceedsInteractionSpread = interaction.RequiresRedistribution
                        && furthest.DistanceMm > (tuning.SupportSpacingThresholdMm * 0.35f);

                    // For balanced force placement: identify the most overloaded support
                    // and prefer placing the new support to relieve it.
                    var balancedPlacementTarget = furthest;
                    if (supportIndices.Count >= 2 && supportForces.Count >= 2)
                    {
                        var mostOverloadedSlot = -1;
                        var worstRatio = 0f;
                        for (var slot = 0; slot < supportForces.Count; slot++)
                        {
                            var metric = supportForces[slot];
                            var capacity = ComputeCapacity(supportPoints[supportForces[slot].SupportIndex], tuning);
                            var peelRatio = capacity > 0f ? metric.VerticalPullForce / capacity : 0f;
                            var crushRatio2 = tuning.CrushForceThreshold > 0f ? metric.CompressiveForce / tuning.CrushForceThreshold : 0f;
                            var angularRatio2 = tuning.MaxAngularForce > 0f ? metric.AngularForce / tuning.MaxAngularForce : 0f;
                            var ratio = MathF.Max(peelRatio, MathF.Max(crushRatio2, angularRatio2));
                            if (ratio > worstRatio)
                            {
                                worstRatio = ratio;
                                mostOverloadedSlot = slot;
                            }
                        }

                        if (mostOverloadedSlot >= 0 && worstRatio > 0.5f)
                        {
                            var overloadedSupportIndex = supportForces[mostOverloadedSlot].SupportIndex;
                            balancedPlacementTarget = FindFurthestPixelFromSupports(
                                unsupportedPixels,
                                [overloadedSupportIndex],
                                supportPoints,
                                bedWidthMm,
                                bedDepthMm,
                                pixelWidth,
                                pixelHeight);
                        }
                    }
                    var strictSupportIndices = FindSupportingSupportsForPixelSet(
                        islandPixelSet,
                        island.SliceHeightMm,
                        supportPoints,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm,
                        includeNeighborPixels: false,
                        minimumSupportY: localSupportMinY);
                    var islandWouldLoseSupportOnMerge = strictSupportIndices.Count == 0;

                    if (!isTipIsland)
                    {
                        if (layerIndex < Math.Max(1, layerIslands.Length / 4))
                            break;

                        var pixelAreaMm2 = (bedWidthMm / pixelWidth) * (bedDepthMm / pixelHeight);
                        var unsupportedAreaMm2 = unsupportedPixels.Count * pixelAreaMm2;
                        var baseDesiredSupports = ComputeDesiredSupportsForIslandArea(
                            unsupportedAreaMm2,
                            tuning.SupportSpacingThresholdMm);
                        var heightRiskMultiplier = 1f + (tuning.HeightBias * (1f - layerProgress));
                        var heightScaledDesiredSupports = Math.Max(
                            1,
                            (int)MathF.Ceiling(baseDesiredSupports * heightRiskMultiplier));
                        if (supportIndices.Count >= heightScaledDesiredSupports)
                            break;
                    }

                    if (islandWouldLoseSupportOnMerge && isTipIsland)
                    {
                        var centroid = ComputePixelCentroidMm(
                            unsupportedPixels,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight);
                        var addResult = TryAddSupportAtBestLayer(
                            supportPoints,
                            maxSupportPoints,
                            unsupportedPixels,
                            layerPixelSets,
                            lowestLayerByPixel,
                            layerIndex,
                            centroid.XMm,
                            centroid.ZMm,
                            SupportSize.Light,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            layerHeightMm,
                            tuning.SupportSpacingThresholdMm,
                            tuning);
                        if (addResult == SupportPlacementResult.CapReached)
                            return changed;
                        if (addResult == SupportPlacementResult.NoValidPlacement)
                            break;
                        changed = true;
                        additionsThisIsland++;
                        continue;
                    }

                    if (!isOverloaded && !exceedsCrushForce && !exceedsAngularForce && !exceedsSpacing && !exceedsInteractionSpread)
                        break;

                    var loadRatio = combinedCapacity <= 0f ? 2f : verticalPullForce / combinedCapacity;
                    var crushRatio = maxCompressiveForce / MathF.Max(tuning.CrushForceThreshold, 0.1f);
                    var angularRatio = maxAngularForce / MathF.Max(tuning.MaxAngularForce, 0.1f);
                    var overloadRatio = MathF.Max(loadRatio, MathF.Max(crushRatio, angularRatio));
                    var newSize = overloadRatio > 1.8f
                        ? SupportSize.Heavy
                        : overloadRatio > 1.25f
                            ? SupportSize.Medium
                            : SupportSize.Light;

                    var minimumPlacementDistanceMm = exceedsSpacing
                        ? tuning.SupportSpacingThresholdMm
                        : Math.Min(tuning.SupportSpacingThresholdMm * 0.5f, 4f);

                    // Use spacing-driven target (furthest from all) for pure spacing violations;
                    // use force-balanced target otherwise to reduce peak forces.
                    var placementTarget = exceedsSpacing ? furthest : balancedPlacementTarget;

                    var reinforceAddResult = TryAddSupportAtBestLayer(
                        supportPoints,
                        maxSupportPoints,
                        unsupportedPixels,
                        layerPixelSets,
                        lowestLayerByPixel,
                        layerIndex,
                        placementTarget.XMm,
                        placementTarget.ZMm,
                        newSize,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm,
                        minimumPlacementDistanceMm,
                        tuning);
                    if (reinforceAddResult == SupportPlacementResult.CapReached)
                        return changed;
                    if (reinforceAddResult == SupportPlacementResult.NoValidPlacement)
                        break;
                    changed = true;
                    additionsThisIsland++;
                    if (!isTipIsland)
                        break;
                }
            }
        }

        return changed;
    }

    private static bool AddSupportAtBestLayer(
        List<SupportPoint> supportPoints,
        List<(int X, int Y)> candidatePixels,
        HashSet<(int Column, int Row)>?[] layerPixelSets,
        IReadOnlyDictionary<(int Column, int Row), int> lowestLayerByPixel,
        int currentLayerIndex,
        float targetXMm,
        float targetZMm,
        SupportSize size,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        float minimumPlacementDistanceMm,
        AutoSupportTuning tuning)
    {
        if (candidatePixels.Count == 0)
            return false;

        (int X, int Y) bestPixel = default;
        var bestLayer = int.MaxValue;
        var bestScore = float.MaxValue;
        var foundPlacement = false;
        var minimumSpacingSq = minimumPlacementDistanceMm * minimumPlacementDistanceMm;

        foreach (var pixel in candidatePixels)
        {
            var candidateLayer = lowestLayerByPixel.TryGetValue((pixel.X, pixel.Y), out var lowestLayer)
                ? Math.Clamp(lowestLayer, 0, currentLayerIndex)
                : currentLayerIndex;
            var xMm = ColumnToX(pixel.X, bedWidthMm, pixelWidth);
            var zMm = RowToZ(pixel.Y, bedDepthMm, pixelHeight);
            var dx = xMm - targetXMm;
            var dz = zMm - targetZMm;
            var targetDistSq = (dx * dx) + (dz * dz);
            var nearestSupportDistSq = ComputeNearestSupportDistanceSq(
                supportPoints,
                xMm,
                zMm);
            var satisfiesSpacing = minimumPlacementDistanceMm <= 0f || nearestSupportDistSq >= minimumSpacingSq;
            var layerPixels = layerPixelSets[candidateLayer];

            if (tuning.AccessibilityEnabled && layerPixels != null)
            {
                if (!HasPlacementAccessibility(
                        layerPixels,
                        pixel.X,
                        pixel.Y,
                        tuning.AccessibilityScanRadiusPx,
                        tuning.AccessibilityMinOpenDirections))
                    continue;
            }

            if (satisfiesSpacing)
            {
                var cosmeticPenalty = 0f;
                if (tuning.SurfaceQualityWeight > 0f && layerPixels != null)
                {
                    cosmeticPenalty = tuning.SurfaceQualityWeight * ComputeSurfacePenalty(
                        layerPixels,
                        pixel.X,
                        pixel.Y,
                        tuning.SurfaceQualitySearchRadiusPx);
                }

                var score = targetDistSq + cosmeticPenalty;
                if (!foundPlacement
                    || candidateLayer < bestLayer
                    || (candidateLayer == bestLayer && score < bestScore))
                {
                    bestLayer = candidateLayer;
                    bestPixel = pixel;
                    bestScore = score;
                    foundPlacement = true;
                }
            }
        }

        if (!foundPlacement)
            return false;

        var yMm = (bestLayer * layerHeightMm) + (layerHeightMm * 0.5f);
        var xPlacementMm = ColumnToX(bestPixel.X, bedWidthMm, pixelWidth);
        var zPlacementMm = RowToZ(bestPixel.Y, bedDepthMm, pixelHeight);

        supportPoints.Add(new SupportPoint(
            new Vec3(xPlacementMm, yMm, zPlacementMm),
            GetTipRadiusMm(size, tuning),
            new Vec3(0f, 0f, 0f),
            size));
        return true;
    }

    private static SupportPlacementResult TryAddSupportAtBestLayer(
        List<SupportPoint> supportPoints,
        int maxSupportPoints,
        List<(int X, int Y)> candidatePixels,
        HashSet<(int Column, int Row)>?[] layerPixelSets,
        IReadOnlyDictionary<(int Column, int Row), int> lowestLayerByPixel,
        int currentLayerIndex,
        float targetXMm,
        float targetZMm,
        SupportSize size,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        float minimumPlacementDistanceMm,
        AutoSupportTuning tuning)
    {
        if (supportPoints.Count >= maxSupportPoints)
            return SupportPlacementResult.CapReached;

        return AddSupportAtBestLayer(
            supportPoints,
            candidatePixels,
            layerPixelSets,
            lowestLayerByPixel,
            currentLayerIndex,
            targetXMm,
            targetZMm,
            size,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm,
            minimumPlacementDistanceMm,
            tuning)
            ? SupportPlacementResult.Added
            : SupportPlacementResult.NoValidPlacement;
    }

    private static float ComputeNearestSupportDistanceSq(
        IReadOnlyList<SupportPoint> supportPoints,
        float xMm,
        float zMm)
    {
        if (supportPoints.Count == 0)
            return float.MaxValue;

        var nearestDistSq = float.MaxValue;
        foreach (var support in supportPoints)
        {
            var dx = support.Position.X - xMm;
            var dz = support.Position.Z - zMm;
            var distSq = (dx * dx) + (dz * dz);
            if (distSq < nearestDistSq)
                nearestDistSq = distSq;
        }

        return nearestDistSq;
    }

    private static void RedistributeSupportHeights(
        List<SupportPoint> supportPoints,
        HashSet<(int Column, int Row)>?[] layerPixelSets,
        HashSet<(int Column, int Row)>?[] prevLayerPixels,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        int layerCount)
    {
        if (supportPoints.Count < 12 || layerCount <= 1)
            return;

        // Precompute which layers have at least one pixel that is new relative to the
        // layer below (a "tip pixel"). Redistribution should only target these layers -
        // promoting a support into a layer with no new overhangs would place it inside
        // the solid interior of an upright body, which is incorrect.
        var layerHasTipPixels = new bool[layerCount];
        for (var l = 1; l < layerCount; l++)
        {
            var lPixels = layerPixelSets[l];
            var lPrev = prevLayerPixels[l];
            if (lPixels == null || lPrev == null)
                continue;
            foreach (var px in lPixels)
            {
                if (!lPrev.Contains(px))
                {
                    layerHasTipPixels[l] = true;
                    break;
                }
            }
        }

        var bottomQuarterLayer = Math.Max(1, layerCount / 4);
        var bottomThresholdY = bottomQuarterLayer * layerHeightMm;
        var maxPromotionLayer = Math.Max(0, (layerCount - 1) / 2);
        var bottomIndices = new List<int>();
        for (var i = 0; i < supportPoints.Count; i++)
        {
            if (supportPoints[i].Position.Y <= bottomThresholdY)
                bottomIndices.Add(i);
        }

        var maxBottomCount = Math.Max(0, (supportPoints.Count - 1) / 2);
        if (bottomIndices.Count <= maxBottomCount)
            return;

        var promoteNeeded = bottomIndices.Count - maxBottomCount;
        foreach (var supportIndex in bottomIndices)
        {
            if (promoteNeeded <= 0)
                break;

            var support = supportPoints[supportIndex];
            var col = (int)MathF.Floor(((support.Position.X + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth);
            var row = (int)MathF.Floor((((bedDepthMm * 0.5f) - support.Position.Z) / bedDepthMm) * pixelHeight);

            var currentLayer = Math.Clamp((int)MathF.Floor(support.Position.Y / layerHeightMm), 0, layerCount - 1);
            var targetLayer = -1;
            for (var layer = maxPromotionLayer; layer > currentLayer; layer--)
            {
                // Only redistribute to layers that have genuine overhang activity -
                // layers whose cross-section contains pixels that did not exist in the
                // layer below. This prevents moving supports into the solid interior of
                // an upright model (e.g. an axis-aligned box) while still allowing
                // redistribution for models with sloping or expanding geometry.
                if (!layerHasTipPixels[layer])
                    continue;

                var pixels = layerPixelSets[layer];
                if (pixels != null
                    && pixels.Contains((col, row))
                    && IsBoundaryPixel(pixels, col, row))
                {
                    targetLayer = layer;
                    break;
                }
            }

            if (targetLayer < 0)
                continue;

            var yMm = (targetLayer * layerHeightMm) + (layerHeightMm * 0.5f);
            supportPoints[supportIndex] = support with
            {
                Position = new Vec3(support.Position.X, yMm, support.Position.Z),
            };
            promoteNeeded--;
        }
    }

    private static bool IsNearBoundaryPixel(
        HashSet<(int Column, int Row)> pixels,
        int column,
        int row,
        int radius = 2)
    {
        for (var dr = -radius; dr <= radius; dr++)
        {
            for (var dc = -radius; dc <= radius; dc++)
            {
                var candidateColumn = column + dc;
                var candidateRow = row + dr;
                if (!pixels.Contains((candidateColumn, candidateRow)))
                    continue;

                if (IsBoundaryPixel(pixels, candidateColumn, candidateRow))
                    return true;
            }
        }

        return false;
    }

    private static bool IsBoundaryPixel(
        HashSet<(int Column, int Row)> pixels,
        int column,
        int row)
    {
        for (var dr = -1; dr <= 1; dr++)
        {
            for (var dc = -1; dc <= 1; dc++)
            {
                if (dc == 0 && dr == 0)
                    continue;

                if (!pixels.Contains((column + dc, row + dr)))
                    return true;
            }
        }

        return false;
    }

    private static void PopulateSupportForces(
        List<TipIsland>[] layerIslands,
        HashSet<(int Column, int Row)>?[] prevLayerPixels,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        AutoSupportTuning tuning,
        float orientationRiskMultiplier)
    {
        if (supportPoints.Count == 0)
            return;

        var accumulatedForces = new Vec3[supportPoints.Count];
        var layerForceBySupport = new Dictionary<int, MutableLayerForce>[supportPoints.Count];
        for (var i = 0; i < layerForceBySupport.Length; i++)
            layerForceBySupport[i] = new Dictionary<int, MutableLayerForce>();

        var mergeLock = new object();
        var pixelAreaMm2 = (bedWidthMm / pixelWidth) * (bedDepthMm / pixelHeight);
        var voxelVolumeMm3 = pixelAreaMm2 * layerHeightMm;
        var gravitationalAcceleration = 9.81f;

        Parallel.ForEach(
            Partitioner.Create(0, layerIslands.Length),
            FullCoreParallelOptions,
            () => new SupportLayerForceAccumulator(supportPoints.Count),
            (range, _, localState) =>
            {
                for (var layerIndex = range.Item1; layerIndex < range.Item2; layerIndex++)
                {
                    var prevPixels = prevLayerPixels[layerIndex];
                    foreach (var island in layerIslands[layerIndex])
                    {
                        var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                        if (unsupportedPixels.Count == 0)
                            continue;
                        var unsupportedPixelSet = unsupportedPixels.ToHashSet();

                        var supportIndices = FindSupportingSupportsForPixelSet(
                            unsupportedPixelSet,
                            island.SliceHeightMm,
                            supportPoints,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            layerHeightMm);

                        if (supportIndices.Count == 0)
                            continue;

                        var layerMassG = island.PixelCoords.Count * voxelVolumeMm3 * (tuning.ResinDensityGPerMl / 1000f);
                        var layerWeightN = layerMassG * gravitationalAcceleration;
                        var perSupportWeight = layerWeightN / supportIndices.Count;

                        var supportForces = EvaluateSupportForces(
                            unsupportedPixels,
                            supportIndices,
                            supportPoints,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            tuning,
                            island,
                            prevPixels: prevPixels,
                            layerProgress: layerIslands.Length <= 1
                                ? 1f
                                : layerIndex / (float)(layerIslands.Length - 1),
                                        orientationRiskMultiplier: orientationRiskMultiplier);

                        foreach (var metric in supportForces)
                        {
                            localState.AccumulatedForces[metric.SupportIndex] = localState.AccumulatedForces[metric.SupportIndex] + metric.PullVector;

                            if (!localState.LayerForcesBySupport[metric.SupportIndex].TryGetValue(layerIndex, out var layerForce))
                            {
                                layerForce = new MutableLayerForce(layerIndex, island.SliceHeightMm);
                                localState.LayerForcesBySupport[metric.SupportIndex][layerIndex] = layerForce;
                            }

                            layerForce.Gravity = layerForce.Gravity + new Vec3(0f, -perSupportWeight, 0f);
                            layerForce.Peel = layerForce.Peel + metric.PeelVector;
                            layerForce.Rotation = layerForce.Rotation + metric.RotationVector;
                        }
                    }
                }

                return localState;
            },
            localState =>
            {
                lock (mergeLock)
                {
                    for (var i = 0; i < localState.AccumulatedForces.Length; i++)
                    {
                        accumulatedForces[i] = accumulatedForces[i] + localState.AccumulatedForces[i];

                        foreach (var entry in localState.LayerForcesBySupport[i])
                        {
                            if (!layerForceBySupport[i].TryGetValue(entry.Key, out var mergedLayer))
                            {
                                layerForceBySupport[i][entry.Key] = entry.Value;
                                continue;
                            }

                            mergedLayer.Gravity = mergedLayer.Gravity + entry.Value.Gravity;
                            mergedLayer.Peel = mergedLayer.Peel + entry.Value.Peel;
                            mergedLayer.Rotation = mergedLayer.Rotation + entry.Value.Rotation;
                        }
                    }
                }
            });

        for (var i = 0; i < supportPoints.Count; i++)
        {
            var layerForces = layerForceBySupport[i]
                .Values
                .OrderBy(layer => layer.LayerIndex)
                .Select(layer => new SupportLayerForce(
                    layer.LayerIndex,
                    layer.SliceHeightMm,
                    layer.Gravity,
                    layer.Peel,
                    layer.Rotation,
                    layer.Gravity + layer.Peel + layer.Rotation))
                .ToArray();

            supportPoints[i] = supportPoints[i] with
            {
                PullForce = accumulatedForces[i],
                LayerForces = layerForces,
            };
        }
    }

    private static List<(int X, int Y)> GetUnsupportedPixels(
        TipIsland island,
        HashSet<(int Column, int Row)>? prevPixels)
    {
        if (prevPixels == null)
            return island.PixelCoords;

        var unsupported = new List<(int X, int Y)>();
        foreach (var pixel in island.PixelCoords)
        {
            if (!prevPixels.Contains(pixel))
                unsupported.Add(pixel);
        }

        return unsupported;
    }

    private static bool IsTipIsland(
        IEnumerable<(int X, int Y)> pixels,
        HashSet<(int Column, int Row)>? prevPixels)
    {
        if (prevPixels == null)
            return true;

        foreach (var pixel in pixels)
        {
            if (prevPixels.Contains(pixel))
                return false;
        }

        return true;
    }

    private static int ComputeDesiredSupportsForIslandArea(float areaMm2, float supportSpacingThresholdMm)
    {
        var spacing = MathF.Max(supportSpacingThresholdMm, 0.1f);
        var coverageAreaMm2 = MathF.PI * spacing * spacing;
        return Math.Max(1, (int)MathF.Ceiling(areaMm2 / coverageAreaMm2));
    }

    private static PixelCentroid ComputePixelCentroidMm(
        List<(int X, int Y)> pixels,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        var sumX = 0f;
        var sumZ = 0f;
        foreach (var (x, y) in pixels)
        {
            sumX += ColumnToX(x, bedWidthMm, pixelWidth);
            sumZ += RowToZ(y, bedDepthMm, pixelHeight);
        }

        return new PixelCentroid(sumX / pixels.Count, sumZ / pixels.Count);
    }

    private static List<SupportForceMetric> EvaluateSupportForces(
        List<(int X, int Y)> unsupportedPixels,
        List<int> supportIndices,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        AutoSupportTuning tuning,
        TipIsland? island = null,
        float areaGrowthRatio = 0f,
        HashSet<(int Column, int Row)>? prevPixels = null,
        float layerProgress = 1f,
        float orientationRiskMultiplier = 1f)
    {
        var pixelAreaMm2 = (bedWidthMm / pixelWidth) * (bedDepthMm / pixelHeight);
        var pixelForce = pixelAreaMm2 * tuning.PeelForceMultiplier;

        // Item 1: Suction multiplier for enclosed regions
        var suctionMultiplier = 1f;
        if (island is { HasEnclosedRegion: true } && tuning.SuctionMultiplier > 1f)
            suctionMultiplier = 1f + ((tuning.SuctionMultiplier - 1f) * island.EnclosureRatio);

        // Item 2: Area growth multiplier for rapid cross-section expansion
        var areaGrowthMultiplier = 1f;
        if (areaGrowthRatio > tuning.AreaGrowthThreshold)
            areaGrowthMultiplier = tuning.AreaGrowthMultiplier;

        // Item 4: Hydrodynamic drag lateral force for thin features
        var dragLateralForce = 0f;
        if (island != null && tuning.DragCoefficientMultiplier > 0f && island.MinWidthMm < tuning.MinFeatureWidthMm)
        {
            var heightEstimateMm = island.AspectRatio * island.MinWidthMm;
            dragLateralForce = heightEstimateMm * island.MinWidthMm * tuning.DragCoefficientMultiplier;
        }

        // Item 6: Overhang angle sensitivity (bitmap approximation via unsupported ratio)
        var overhangMultiplier = 1f;
        if (island != null && island.PixelCoords.Count > 0)
        {
            var newPixelRatio = Math.Clamp(unsupportedPixels.Count / (float)island.PixelCoords.Count, 0f, 1f);
            overhangMultiplier = 1f + (tuning.OverhangSensitivity * newPixelRatio);
        }

        // Item 17: Dynamic support density by print height (more conservative near base)
        var clampedProgress = Math.Clamp(layerProgress, 0f, 1f);
        var heightMultiplier = 1f + (tuning.HeightBias * (1f - clampedProgress));

        // Item 10: Drainage depth risk for enclosed features where trapped resin has poor venting.
        var drainageMultiplier = 1f;
        if (island is { HasEnclosedRegion: true } && tuning.DrainageDepthForceMultiplier > 0f)
        {
            drainageMultiplier += MathF.Min(2f, island.EnclosureDepthLayers * tuning.DrainageDepthForceMultiplier);
        }

        // Item 9: Bridge/cantilever topology detection
        var topologyMultiplier = 1f;
        if (island != null)
        {
            var topology = AnalyzeIslandTopology(
                unsupportedPixels,
                prevPixels,
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight);

            if (topology.Topology == IslandTopology.Bridge)
            {
                topologyMultiplier = MathF.Max(
                    0.55f,
                    1f - (tuning.BridgeReductionFactor * topology.AnchoredPerimeterRatio));
            }
            else if (topology.Topology == IslandTopology.Cantilever)
            {
                var referenceLength = MathF.Max(tuning.CantileverReferenceLengthMm, 0.1f);
                topologyMultiplier = 1f +
                    ((topology.CantileverLengthMm / referenceLength) * tuning.CantileverMomentMultiplier);
            }
        }

        var islandCenterX = island?.CentroidX ?? 0f;
        var islandCenterZ = island?.CentroidZ ?? 0f;
        if (island == null && unsupportedPixels.Count > 0)
        {
            var fallbackCenter = ComputePixelCentroidMm(
                unsupportedPixels,
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight);
            islandCenterX = fallbackCenter.XMm;
            islandCenterZ = fallbackCenter.ZMm;
        }

        // Item 7: Tilt-peel kinematics modeled as a position-dependent gradient.
        var peelKinematicsMultiplier = ComputePeelKinematicsMultiplier(
            islandCenterX,
            islandCenterZ,
            bedWidthMm,
            bedDepthMm,
            tuning);

        var effectivePixelForce = pixelForce
            * suctionMultiplier
            * areaGrowthMultiplier
            * overhangMultiplier
            * heightMultiplier
            * topologyMultiplier
            * peelKinematicsMultiplier
            * drainageMultiplier
            * MathF.Max(1f, orientationRiskMultiplier);
        var supportSlotCount = supportIndices.Count;
        var supportSlots = new SupportPoint[supportSlotCount];
        for (var slot = 0; slot < supportSlotCount; slot++)
            supportSlots[slot] = supportPoints[supportIndices[slot]];

        var pixelCountBySlot = new int[supportSlotCount];
        var sumXBySlot = new float[supportSlotCount];
        var sumZBySlot = new float[supportSlotCount];
        var angularBySlot = new float[supportSlotCount];

        var shouldParallelize = unsupportedPixels.Count >= 256 && supportSlotCount > 1;
        if (shouldParallelize)
        {
            var ranges = BuildWorkRanges(unsupportedPixels.Count);
            var localPixelCounts = new int[ranges.Length][];
            var localSumX = new float[ranges.Length][];
            var localSumZ = new float[ranges.Length][];
            var localAngular = new float[ranges.Length][];

            Parallel.For(0, ranges.Length, FullCoreParallelOptions, rangeIndex =>
            {
                var localCounts = new int[supportSlotCount];
                var localX = new float[supportSlotCount];
                var localZ = new float[supportSlotCount];
                var localAngle = new float[supportSlotCount];

                var (start, end) = ranges[rangeIndex];
                for (var pixelIndex = start; pixelIndex < end; pixelIndex++)
                {
                    var (x, y) = unsupportedPixels[pixelIndex];
                    var xMm = ColumnToX(x, bedWidthMm, pixelWidth);
                    var zMm = RowToZ(y, bedDepthMm, pixelHeight);
                    var nearestSlot = 0;
                    var nearestDistanceSq = float.MaxValue;

                    for (var slot = 0; slot < supportSlotCount; slot++)
                    {
                        var support = supportSlots[slot];
                        var dx = xMm - support.Position.X;
                        var dz = zMm - support.Position.Z;
                        var distanceSq = (dx * dx) + (dz * dz);
                        if (distanceSq < nearestDistanceSq)
                        {
                            nearestDistanceSq = distanceSq;
                            nearestSlot = slot;
                        }
                    }

                    localCounts[nearestSlot]++;
                    localX[nearestSlot] += xMm;
                    localZ[nearestSlot] += zMm;
                    var leverDx = xMm - islandCenterX;
                    var leverDz = zMm - islandCenterZ;
                    var leverArm = MathF.Sqrt((leverDx * leverDx) + (leverDz * leverDz));
                    localAngle[nearestSlot] += effectivePixelForce * leverArm;
                }

                localPixelCounts[rangeIndex] = localCounts;
                localSumX[rangeIndex] = localX;
                localSumZ[rangeIndex] = localZ;
                localAngular[rangeIndex] = localAngle;
            });

            for (var rangeIndex = 0; rangeIndex < ranges.Length; rangeIndex++)
            {
                var localCounts = localPixelCounts[rangeIndex];
                var localX = localSumX[rangeIndex];
                var localZ = localSumZ[rangeIndex];
                var localAngle = localAngular[rangeIndex];
                for (var slot = 0; slot < supportSlotCount; slot++)
                {
                    pixelCountBySlot[slot] += localCounts[slot];
                    sumXBySlot[slot] += localX[slot];
                    sumZBySlot[slot] += localZ[slot];
                    angularBySlot[slot] += localAngle[slot];
                }
            }
        }
        else
        {
            for (var pixelIndex = 0; pixelIndex < unsupportedPixels.Count; pixelIndex++)
            {
                var (x, y) = unsupportedPixels[pixelIndex];
                var xMm = ColumnToX(x, bedWidthMm, pixelWidth);
                var zMm = RowToZ(y, bedDepthMm, pixelHeight);
                var nearestSlot = 0;
                var nearestDistanceSq = float.MaxValue;

                for (var slot = 0; slot < supportSlotCount; slot++)
                {
                    var support = supportSlots[slot];
                    var dx = xMm - support.Position.X;
                    var dz = zMm - support.Position.Z;
                    var distanceSq = (dx * dx) + (dz * dz);
                    if (distanceSq < nearestDistanceSq)
                    {
                        nearestDistanceSq = distanceSq;
                        nearestSlot = slot;
                    }
                }

                pixelCountBySlot[nearestSlot]++;
                sumXBySlot[nearestSlot] += xMm;
                sumZBySlot[nearestSlot] += zMm;
                var leverDx = xMm - islandCenterX;
                var leverDz = zMm - islandCenterZ;
                var leverArm = MathF.Sqrt((leverDx * leverDx) + (leverDz * leverDz));
                angularBySlot[nearestSlot] += effectivePixelForce * leverArm;
            }
        }

        var metrics = new List<SupportForceMetric>(supportIndices.Count);
        for (var slot = 0; slot < supportSlotCount; slot++)
        {
            var supportIndex = supportIndices[slot];
            var pixelCount = pixelCountBySlot[slot];
            if (pixelCount == 0)
            {
                metrics.Add(new SupportForceMetric(
                    supportIndex,
                    new Vec3(0f, 0f, 0f),
                    new Vec3(0f, 0f, 0f),
                    new Vec3(0f, 0f, 0f),
                    0f,
                    0f,
                    0f));
                continue;
            }

            var support = supportPoints[supportIndex];
            var verticalPullForce = pixelCount * effectivePixelForce;
            var centroidX = sumXBySlot[slot] / pixelCount;
            var centroidZ = sumZBySlot[slot] / pixelCount;
            var baseLateralX = (centroidX - support.Position.X) * 0.35f * MathF.Sqrt(MathF.Max(verticalPullForce, 0.01f));
            var baseLateralZ = (centroidZ - support.Position.Z) * 0.35f * MathF.Sqrt(MathF.Max(verticalPullForce, 0.01f));

            // Item 4: Add hydrodynamic drag as lateral force distributed across supports
            var lateralX = baseLateralX;
            var lateralZ = baseLateralZ;
            if (dragLateralForce > 0f && supportIndices.Count > 0)
            {
                var perSupportDrag = dragLateralForce / supportIndices.Count;
                var dragDirX = centroidX - support.Position.X;
                var dragDirZ = centroidZ - support.Position.Z;
                var dragDirLen = MathF.Sqrt((dragDirX * dragDirX) + (dragDirZ * dragDirZ));
                if (dragDirLen > 0.001f)
                {
                    lateralX += (dragDirX / dragDirLen) * perSupportDrag;
                    lateralZ += (dragDirZ / dragDirLen) * perSupportDrag;
                }
            }

            var crushForce = angularBySlot[slot] / MathF.Max(support.RadiusMm, 0.1f);
            var signedVerticalForce = verticalPullForce - crushForce;
            var compressiveForce = MathF.Max(0f, -signedVerticalForce);
            var peelVector = new Vec3(0f, verticalPullForce, 0f);
            var rotationVector = new Vec3(lateralX, -compressiveForce, lateralZ);
            var pullVector = new Vec3(lateralX, signedVerticalForce, lateralZ);

            metrics.Add(new SupportForceMetric(
                supportIndex,
                pullVector,
                peelVector,
                rotationVector,
                verticalPullForce,
                compressiveForce,
                angularBySlot[slot]));
        }

        return metrics;
    }

    private static List<int> FindSupportingSupportsForPixels(
        List<(int X, int Y)> unsupportedPixels,
        float sliceHeightMm,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        bool includeNeighborPixels = true,
        float minimumSupportY = float.NegativeInfinity)
    {
        var pixelSet = unsupportedPixels.ToHashSet();
        return FindSupportingSupportsForPixelSet(
            pixelSet,
            sliceHeightMm,
            supportPoints,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm,
                includeNeighborPixels,
                minimumSupportY);
    }

    private static List<int> FindSupportingSupportsForPixelSet(
        HashSet<(int X, int Y)> pixelSet,
        float sliceHeightMm,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        bool includeNeighborPixels = true,
        float minimumSupportY = float.NegativeInfinity)
    {
        var indices = new List<int>();
        var maxY = sliceHeightMm + (layerHeightMm * 0.5f);

        var shouldParallelize = supportPoints.Count >= 1024;
        if (!shouldParallelize)
        {
            for (var i = 0; i < supportPoints.Count; i++)
            {
                var support = supportPoints[i];
                if (support.Position.Y > maxY || support.Position.Y < minimumSupportY)
                    continue;

                if (IsSupportInsidePixelSet(
                    support.Position,
                    pixelSet,
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    includeNeighborPixels))
                    indices.Add(i);
            }

            return indices;
        }

        var matched = new bool[supportPoints.Count];
        var ranges = BuildWorkRanges(supportPoints.Count);
        Parallel.For(0, ranges.Length, FullCoreParallelOptions, rangeIndex =>
        {
            var (start, end) = ranges[rangeIndex];
            for (var i = start; i < end; i++)
            {
                var support = supportPoints[i];
                if (support.Position.Y > maxY || support.Position.Y < minimumSupportY)
                    continue;

                if (IsSupportInsidePixelSet(
                    support.Position,
                    pixelSet,
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    includeNeighborPixels))
                    matched[i] = true;
            }
        });

        for (var i = 0; i < matched.Length; i++)
        {
            if (matched[i])
                indices.Add(i);
        }

        return indices;
    }

    private static bool IsSupportInsidePixelSet(
        Vec3 supportPosition,
        HashSet<(int X, int Y)> pixelSet,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        bool includeNeighborPixels)
    {
        var col = (int)MathF.Floor(((supportPosition.X + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth);
        var row = (int)MathF.Floor((((bedDepthMm * 0.5f) - supportPosition.Z) / bedDepthMm) * pixelHeight);

        if (!includeNeighborPixels)
            return pixelSet.Contains((col, row));

        for (var dy = -1; dy <= 1; dy++)
        {
            for (var dx = -1; dx <= 1; dx++)
            {
                if (pixelSet.Contains((col + dx, row + dy)))
                    return true;
            }
        }

        return false;
    }

    private static FurthestPointCandidate FindFurthestPixelFromSupports(
        List<(int X, int Y)> pixels,
        List<int> supportIndices,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        if (pixels.Count == 0)
            return new FurthestPointCandidate(0f, 0f, 0f);

        if (pixels.Count < 256 || supportIndices.Count < 2)
        {
            var bestDistSerial = -1f;
            var bestXSerial = 0f;
            var bestZSerial = 0f;

            foreach (var (x, y) in pixels)
            {
                var xMm = ColumnToX(x, bedWidthMm, pixelWidth);
                var zMm = RowToZ(y, bedDepthMm, pixelHeight);
                var nearest = float.MaxValue;

                foreach (var supportIndex in supportIndices)
                {
                    var support = supportPoints[supportIndex];
                    var dx = support.Position.X - xMm;
                    var dz = support.Position.Z - zMm;
                    var d = MathF.Sqrt((dx * dx) + (dz * dz));
                    nearest = MathF.Min(nearest, d);
                }

                if (nearest > bestDistSerial)
                {
                    bestDistSerial = nearest;
                    bestXSerial = xMm;
                    bestZSerial = zMm;
                }
            }

            return new FurthestPointCandidate(bestXSerial, bestZSerial, bestDistSerial);
        }

        var ranges = BuildWorkRanges(pixels.Count);
        var bestDistByRange = new float[ranges.Length];
        var bestXByRange = new float[ranges.Length];
        var bestZByRange = new float[ranges.Length];

        Parallel.For(0, ranges.Length, FullCoreParallelOptions, rangeIndex =>
        {
            var localBestDist = -1f;
            var localBestX = 0f;
            var localBestZ = 0f;
            var (start, end) = ranges[rangeIndex];

            for (var pixelIndex = start; pixelIndex < end; pixelIndex++)
            {
                var (x, y) = pixels[pixelIndex];
                var xMm = ColumnToX(x, bedWidthMm, pixelWidth);
                var zMm = RowToZ(y, bedDepthMm, pixelHeight);
                var nearest = float.MaxValue;

                foreach (var supportIndex in supportIndices)
                {
                    var support = supportPoints[supportIndex];
                    var dx = support.Position.X - xMm;
                    var dz = support.Position.Z - zMm;
                    var d = MathF.Sqrt((dx * dx) + (dz * dz));
                    nearest = MathF.Min(nearest, d);
                }

                if (nearest > localBestDist)
                {
                    localBestDist = nearest;
                    localBestX = xMm;
                    localBestZ = zMm;
                }
            }

            bestDistByRange[rangeIndex] = localBestDist;
            bestXByRange[rangeIndex] = localBestX;
            bestZByRange[rangeIndex] = localBestZ;
        });

        var bestDist = -1f;
        var bestX = 0f;
        var bestZ = 0f;

        for (var rangeIndex = 0; rangeIndex < ranges.Length; rangeIndex++)
        {
            var candidateDist = bestDistByRange[rangeIndex];
            if (candidateDist > bestDist)
            {
                bestDist = candidateDist;
                bestX = bestXByRange[rangeIndex];
                bestZ = bestZByRange[rangeIndex];
            }
        }

        return new FurthestPointCandidate(bestX, bestZ, bestDist);
    }

    private static (int Start, int End)[] BuildWorkRanges(int itemCount)
    {
        if (itemCount <= 0)
            return [];

        var targetRangeCount = Math.Max(1, Environment.ProcessorCount * 4);
        var rangeCount = Math.Min(itemCount, targetRangeCount);
        var baseSize = itemCount / rangeCount;
        var remainder = itemCount % rangeCount;
        var ranges = new (int Start, int End)[rangeCount];
        var start = 0;

        for (var i = 0; i < rangeCount; i++)
        {
            var size = baseSize + (i < remainder ? 1 : 0);
            var end = start + size;
            ranges[i] = (start, end);
            start = end;
        }

        return ranges;
    }

    private static float ComputeCapacity(SupportPoint point, AutoSupportTuning tuning)
        => MathF.PI * point.RadiusMm * point.RadiusMm * tuning.ResinStrength;

    private static float ComputePeelKinematicsMultiplier(
        float islandCenterX,
        float islandCenterZ,
        float bedWidthMm,
        float bedDepthMm,
        AutoSupportTuning tuning)
    {
        var progress = tuning.PeelDirection switch
        {
            PeelDirection.XPositive => ((islandCenterX + (bedWidthMm * 0.5f)) / MathF.Max(bedWidthMm, 0.01f)),
            PeelDirection.XNegative => 1f - ((islandCenterX + (bedWidthMm * 0.5f)) / MathF.Max(bedWidthMm, 0.01f)),
            PeelDirection.ZPositive => (((bedDepthMm * 0.5f) - islandCenterZ) / MathF.Max(bedDepthMm, 0.01f)),
            PeelDirection.ZNegative => 1f - (((bedDepthMm * 0.5f) - islandCenterZ) / MathF.Max(bedDepthMm, 0.01f)),
            _ => 0.5f,
        };

        progress = Math.Clamp(progress, 0f, 1f);
        return tuning.PeelStartMultiplier + ((tuning.PeelEndMultiplier - tuning.PeelStartMultiplier) * progress);
    }

    private static IslandTopologyMetrics AnalyzeIslandTopology(
        List<(int X, int Y)> unsupportedPixels,
        HashSet<(int Column, int Row)>? prevPixels,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        if (prevPixels == null || unsupportedPixels.Count == 0)
            return new IslandTopologyMetrics(IslandTopology.Floating, 0f, 0f);

        var anchoredCount = 0;
        var hasWest = false;
        var hasEast = false;
        var hasNorth = false;
        var hasSouth = false;

        var minX = int.MaxValue;
        var maxX = int.MinValue;
        var minY = int.MaxValue;
        var maxY = int.MinValue;
        var minAnchorX = int.MaxValue;
        var maxAnchorX = int.MinValue;
        var minAnchorY = int.MaxValue;
        var maxAnchorY = int.MinValue;

        foreach (var (x, y) in unsupportedPixels)
        {
            if (x < minX) minX = x;
            if (x > maxX) maxX = x;
            if (y < minY) minY = y;
            if (y > maxY) maxY = y;

            var anchored = false;
            if (prevPixels.Contains((x - 1, y)))
            {
                hasWest = true;
                anchored = true;
            }
            if (prevPixels.Contains((x + 1, y)))
            {
                hasEast = true;
                anchored = true;
            }
            if (prevPixels.Contains((x, y - 1)))
            {
                hasNorth = true;
                anchored = true;
            }
            if (prevPixels.Contains((x, y + 1)))
            {
                hasSouth = true;
                anchored = true;
            }

            if (!anchored)
                continue;

            anchoredCount++;
            if (x < minAnchorX) minAnchorX = x;
            if (x > maxAnchorX) maxAnchorX = x;
            if (y < minAnchorY) minAnchorY = y;
            if (y > maxAnchorY) maxAnchorY = y;
        }

        if (anchoredCount == 0)
            return new IslandTopologyMetrics(IslandTopology.Floating, 0f, 0f);

        var sides = 0;
        if (hasWest) sides++;
        if (hasEast) sides++;
        if (hasNorth) sides++;
        if (hasSouth) sides++;

        var anchoredRatio = anchoredCount / (float)unsupportedPixels.Count;
        if (sides >= 2)
            return new IslandTopologyMetrics(IslandTopology.Bridge, anchoredRatio, 0f);

        var pixelWidthMm = bedWidthMm / MathF.Max(1, pixelWidth);
        var pixelDepthMm = bedDepthMm / MathF.Max(1, pixelHeight);

        var cantileverLengthMm = 0f;
        if (hasWest)
            cantileverLengthMm = MathF.Max(0f, (maxX - minAnchorX) * pixelWidthMm);
        else if (hasEast)
            cantileverLengthMm = MathF.Max(0f, (maxAnchorX - minX) * pixelWidthMm);
        else if (hasNorth)
            cantileverLengthMm = MathF.Max(0f, (maxY - minAnchorY) * pixelDepthMm);
        else if (hasSouth)
            cantileverLengthMm = MathF.Max(0f, (maxAnchorY - minY) * pixelDepthMm);

        return new IslandTopologyMetrics(IslandTopology.Cantilever, anchoredRatio, cantileverLengthMm);
    }

    private static SupportInteractionMetrics AnalyzeSupportInteraction(
        IReadOnlyList<int> supportIndices,
        IReadOnlyList<SupportPoint> supportPoints,
        float spacingThresholdMm,
        bool enabled)
    {
        if (!enabled || supportIndices.Count < 2)
            return new SupportInteractionMetrics(1f, 1f, false);

        var pairCount = 0;
        var closePairCount = 0;
        var sumDistance = 0f;
        for (var i = 0; i < supportIndices.Count; i++)
        {
            var a = supportPoints[supportIndices[i]].Position;
            for (var j = i + 1; j < supportIndices.Count; j++)
            {
                var b = supportPoints[supportIndices[j]].Position;
                var dx = a.X - b.X;
                var dz = a.Z - b.Z;
                var d = MathF.Sqrt((dx * dx) + (dz * dz));
                sumDistance += d;
                pairCount++;
                if (d <= spacingThresholdMm * 0.6f)
                    closePairCount++;
            }
        }

        if (pairCount == 0)
            return new SupportInteractionMetrics(1f, 1f, false);

        var avgDistance = sumDistance / pairCount;
        var closeRatio = closePairCount / (float)pairCount;
        var capacityMultiplier = 1f;
        var loadMultiplier = 1f;

        if (avgDistance <= spacingThresholdMm * 0.75f)
            capacityMultiplier += 0.08f;

        if (closeRatio > 0.45f)
            loadMultiplier += 0.1f;

        var requiresRedistribution = closeRatio > 0.6f;
        return new SupportInteractionMetrics(capacityMultiplier, loadMultiplier, requiresRedistribution);
    }

    private static bool HasPlacementAccessibility(
        HashSet<(int Column, int Row)> layerPixels,
        int column,
        int row,
        int scanRadius,
        int minOpenDirections)
    {
        if (!layerPixels.Contains((column, row)))
            return false;

        var openDirections = 0;
        foreach (var (dx, dy) in new[] { (1, 0), (-1, 0), (0, 1), (0, -1) })
        {
            for (var step = 1; step <= scanRadius; step++)
            {
                var nx = column + (dx * step);
                var ny = row + (dy * step);
                if (!layerPixels.Contains((nx, ny)))
                {
                    openDirections++;
                    break;
                }
            }
        }

        return openDirections >= minOpenDirections;
    }

    private static float ComputeSurfacePenalty(
        HashSet<(int Column, int Row)> layerPixels,
        int column,
        int row,
        int maxSearchRadius)
    {
        if (!layerPixels.Contains((column, row)))
            return 1f;

        if (IsBoundaryPixel(layerPixels, column, row))
            return 0f;

        for (var radius = 1; radius <= maxSearchRadius; radius++)
        {
            for (var dy = -radius; dy <= radius; dy++)
            {
                var dx = radius - Math.Abs(dy);
                if (IsBoundaryPixel(layerPixels, column + dx, row + dy)
                    || IsBoundaryPixel(layerPixels, column - dx, row + dy))
                {
                    return radius / (float)Math.Max(maxSearchRadius, 1);
                }
            }
        }

        return 1f;
    }

    private float EstimateOrientationRiskMultiplier(LoadedGeometry geometry, AutoSupportTuning tuning)
    {
        if (geometry.Triangles.Count == 0)
            return 1f;

        var baseScore = ComputeOrientationScore(geometry.Triangles);
        var bestScore = baseScore;
        var candidates = new (float X, float Z)[]
        {
            (MathF.PI / 9f, 0f),
            (-MathF.PI / 9f, 0f),
            (0f, MathF.PI / 9f),
            (0f, -MathF.PI / 9f),
            (MathF.PI / 9f, MathF.PI / 9f),
            (-MathF.PI / 9f, -MathF.PI / 9f),
        };

        foreach (var (rotX, rotZ) in candidates)
        {
            var score = ComputeOrientationScore(geometry.Triangles, rotX, rotZ);
            if (score < bestScore)
                bestScore = score;
        }

        if (bestScore <= 0.0001f)
            return 1f;

        var ratio = baseScore / bestScore;
        if (ratio <= tuning.OrientationRiskThresholdRatio)
            return 1f;

        return Math.Clamp(ratio, 1f, tuning.OrientationRiskForceMultiplierMax);
    }

    private static float ComputeOrientationScore(
        IReadOnlyList<Triangle3D> triangles,
        float rotateX = 0f,
        float rotateZ = 0f)
    {
        var score = 0f;
        var cosX = MathF.Cos(rotateX);
        var sinX = MathF.Sin(rotateX);
        var cosZ = MathF.Cos(rotateZ);
        var sinZ = MathF.Sin(rotateZ);

        foreach (var triangle in triangles)
        {
            var normal = triangle.Normal;
            if (rotateX != 0f)
                normal = new Vec3(normal.X, (normal.Y * cosX) - (normal.Z * sinX), (normal.Y * sinX) + (normal.Z * cosX));
            if (rotateZ != 0f)
                normal = new Vec3((normal.X * cosZ) - (normal.Y * sinZ), (normal.X * sinZ) + (normal.Y * cosZ), normal.Z);

            var downward = MathF.Max(0f, -normal.Y);
            if (downward <= 0f)
                continue;

            var edgeA = triangle.V1 - triangle.V0;
            var edgeB = triangle.V2 - triangle.V0;
            var area = edgeA.Cross(edgeB).Length * 0.5f;
            score += area * downward;
        }

        return score;
    }

    private static bool ApplyLayerAdhesionReinforcement(
        HashSet<(int Column, int Row)>?[] layerPixelSets,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        AutoSupportTuning tuning)
    {
        if (supportPoints.Count == 0 || layerPixelSets.Length == 0)
            return false;

        var changed = false;
        var pixelAreaMm2 = (bedWidthMm / pixelWidth) * (bedDepthMm / pixelHeight);
        var maxLayer = layerPixelSets.Length - 1;

        for (var supportIndex = 0; supportIndex < supportPoints.Count; supportIndex++)
        {
            var support = supportPoints[supportIndex];
            var supportLayerForces = support.LayerForces;
            var expectedLoadN = MathF.Abs(support.PullForce.Y);

            if (supportLayerForces is { Count: > 0 })
            {
                var peelPeak = supportLayerForces.Max(layer => layer.Peel.Y);
                if (peelPeak > expectedLoadN)
                    expectedLoadN = peelPeak;
            }

            if (expectedLoadN <= 0.01f)
                continue;

            var col = (int)MathF.Floor(((support.Position.X + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth);
            var row = (int)MathF.Floor((((bedDepthMm * 0.5f) - support.Position.Z) / bedDepthMm) * pixelHeight);
            if (col < 0 || row < 0 || col >= pixelWidth || row >= pixelHeight)
                continue;

            var startLayer = Math.Clamp((int)MathF.Floor(support.Position.Y / MathF.Max(layerHeightMm, 0.001f)), 0, maxLayer);
            var bottleneckAreaMm2 = float.MaxValue;

            for (var layer = startLayer; layer >= 0; layer--)
            {
                var pixels = layerPixelSets[layer];
                if (pixels == null || !pixels.Contains((col, row)))
                    continue;

                var localCount = 0;
                for (var dy = -1; dy <= 1; dy++)
                {
                    for (var dx = -1; dx <= 1; dx++)
                    {
                        if (pixels.Contains((col + dx, row + dy)))
                            localCount++;
                    }
                }

                if (localCount <= 0)
                    continue;

                var localArea = localCount * pixelAreaMm2;
                if (localArea < bottleneckAreaMm2)
                    bottleneckAreaMm2 = localArea;
            }

            if (float.IsPositiveInfinity(bottleneckAreaMm2) || bottleneckAreaMm2 == float.MaxValue)
                continue;

            var requiredAreaMm2 = expectedLoadN / MathF.Max(tuning.LayerBondStrengthPerMm2, 0.01f);
            requiredAreaMm2 *= MathF.Max(1f, tuning.LayerAdhesionSafetyFactor);

            if (bottleneckAreaMm2 >= requiredAreaMm2)
                continue;

            var upgradedSize = support.Size switch
            {
                SupportSize.Light => SupportSize.Medium,
                SupportSize.Medium => SupportSize.Heavy,
                _ => support.Size,
            };

            if (upgradedSize == support.Size)
                continue;

            supportPoints[supportIndex] = support with
            {
                Size = upgradedSize,
                RadiusMm = GetTipRadiusMm(upgradedSize, tuning),
            };
            changed = true;
        }

        return changed;
    }

    private static List<VoxelRect> ExtractVoxelRects(SliceBitmap bitmap)
    {
        var visited = new bool[bitmap.Pixels.Length];
        var rects = new List<VoxelRect>();

        for (var row = 0; row < bitmap.Height; row++)
        {
            for (var col = 0; col < bitmap.Width; col++)
            {
                var idx = (row * bitmap.Width) + col;
                if (visited[idx] || bitmap.Pixels[idx] == 0)
                    continue;

                var width = 0;
                while (col + width < bitmap.Width)
                {
                    var i = (row * bitmap.Width) + col + width;
                    if (visited[i] || bitmap.Pixels[i] == 0)
                        break;
                    width++;
                }

                var height = 1;
                var canGrow = true;
                while (canGrow && row + height < bitmap.Height)
                {
                    for (var x = 0; x < width; x++)
                    {
                        var i = ((row + height) * bitmap.Width) + col + x;
                        if (visited[i] || bitmap.Pixels[i] == 0)
                        {
                            canGrow = false;
                            break;
                        }
                    }

                    if (canGrow)
                        height++;
                }

                for (var y = 0; y < height; y++)
                {
                    for (var x = 0; x < width; x++)
                        visited[((row + y) * bitmap.Width) + col + x] = true;
                }

                rects.Add(new VoxelRect(col, row, width, height));
            }
        }

        return rects;
    }

    private static List<Triangle3D> BuildMergedVoxelMesh(
        List<VoxelRect>[] layerVoxelRects,
        float bedWidthMm,
        float bedDepthMm,
        float layerHeightMm,
        int pixelWidth,
        int pixelHeight,
        int maxTriangles,
        out bool truncated)
    {
        var triangles = new List<Triangle3D>(Math.Min(maxTriangles, 32_768));
        var activeBlocks = new Dictionary<VoxelRectKey, int>();
        truncated = false;

        for (var layerIndex = 0; layerIndex < layerVoxelRects.Length; layerIndex++)
        {
            var currentRects = layerVoxelRects[layerIndex] ?? [];
            var currentKeys = new HashSet<VoxelRectKey>();

            foreach (var rect in currentRects)
            {
                var key = new VoxelRectKey(rect.X, rect.Y, rect.Width, rect.Height);
                currentKeys.Add(key);
                if (!activeBlocks.ContainsKey(key))
                    activeBlocks[key] = layerIndex;
            }

            var toFinalize = new List<VoxelRectKey>();
            foreach (var pair in activeBlocks)
            {
                if (!currentKeys.Contains(pair.Key))
                    toFinalize.Add(pair.Key);
            }

            foreach (var key in toFinalize)
            {
                var startLayer = activeBlocks[key];
                var endLayer = layerIndex - 1;
                if (!TryAppendVoxelBlock(
                        triangles,
                        key,
                        startLayer,
                        endLayer,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm,
                        maxTriangles))
                {
                    truncated = true;
                    return triangles;
                }

                activeBlocks.Remove(key);
            }
        }

        foreach (var pair in activeBlocks)
        {
            if (!TryAppendVoxelBlock(
                    triangles,
                    pair.Key,
                    pair.Value,
                    layerVoxelRects.Length - 1,
                    bedWidthMm,
                    bedDepthMm,
                    pixelWidth,
                    pixelHeight,
                    layerHeightMm,
                    maxTriangles))
            {
                truncated = true;
                return triangles;
            }
        }

        return triangles;
    }

    private static bool TryAppendVoxelBlock(
        List<Triangle3D> triangles,
        VoxelRectKey key,
        int startLayer,
        int endLayer,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        int maxTriangles)
    {
        if (triangles.Count + TrianglesPerVoxelBlock > maxTriangles)
            return false;

        var x0 = ColumnEdgeToX(key.X, bedWidthMm, pixelWidth);
        var x1 = ColumnEdgeToX(key.X + key.Width, bedWidthMm, pixelWidth);
        var zA = RowEdgeToZ(key.Y, bedDepthMm, pixelHeight);
        var zB = RowEdgeToZ(key.Y + key.Height, bedDepthMm, pixelHeight);
        var z0 = MathF.Min(zA, zB);
        var z1 = MathF.Max(zA, zB);
        var y0 = startLayer * layerHeightMm;
        var y1 = (endLayer + 1) * layerHeightMm;

        var p000 = new Vec3(x0, y0, z0);
        var p001 = new Vec3(x0, y0, z1);
        var p010 = new Vec3(x0, y1, z0);
        var p011 = new Vec3(x0, y1, z1);
        var p100 = new Vec3(x1, y0, z0);
        var p101 = new Vec3(x1, y0, z1);
        var p110 = new Vec3(x1, y1, z0);
        var p111 = new Vec3(x1, y1, z1);

        // Top (+Y)
        triangles.Add(new Triangle3D(p010, p111, p110, new Vec3(0f, 1f, 0f)));
        triangles.Add(new Triangle3D(p010, p011, p111, new Vec3(0f, 1f, 0f)));
        // Bottom (-Y)
        triangles.Add(new Triangle3D(p000, p100, p101, new Vec3(0f, -1f, 0f)));
        triangles.Add(new Triangle3D(p000, p101, p001, new Vec3(0f, -1f, 0f)));
        // Front (+Z)
        triangles.Add(new Triangle3D(p001, p111, p011, new Vec3(0f, 0f, 1f)));
        triangles.Add(new Triangle3D(p001, p101, p111, new Vec3(0f, 0f, 1f)));
        // Back (-Z)
        triangles.Add(new Triangle3D(p000, p010, p110, new Vec3(0f, 0f, -1f)));
        triangles.Add(new Triangle3D(p000, p110, p100, new Vec3(0f, 0f, -1f)));
        // Left (-X)
        triangles.Add(new Triangle3D(p000, p011, p010, new Vec3(-1f, 0f, 0f)));
        triangles.Add(new Triangle3D(p000, p001, p011, new Vec3(-1f, 0f, 0f)));
        // Right (+X)
        triangles.Add(new Triangle3D(p100, p111, p101, new Vec3(1f, 0f, 0f)));
        triangles.Add(new Triangle3D(p100, p110, p111, new Vec3(1f, 0f, 0f)));

        return true;
    }

    private static float ColumnEdgeToX(int columnEdge, float bedWidthMm, int width)
        => ((columnEdge / (float)width) * bedWidthMm) - (bedWidthMm * 0.5f);

    private static float RowEdgeToZ(int rowEdge, float bedDepthMm, int height)
        => (bedDepthMm * 0.5f) - ((rowEdge / (float)height) * bedDepthMm);

    private static List<Triangle3D> BuildSupportSphereMesh(IReadOnlyList<SupportPoint> supportPoints)
    {
        var triangles = new List<Triangle3D>(supportPoints.Count * 144);
        if (supportPoints.Count == 0)
            return triangles;

        var trianglesBySupport = new List<Triangle3D>[supportPoints.Count];
        Parallel.For(0, supportPoints.Count, FullCoreParallelOptions, supportIndex =>
        {
            var support = supportPoints[supportIndex];
            var localTriangles = new List<Triangle3D>(144);
            AppendSphere(localTriangles, support.Position, support.RadiusMm, latSegments: 8, lonSegments: 12);
            trianglesBySupport[supportIndex] = localTriangles;
        });

        for (var supportIndex = 0; supportIndex < trianglesBySupport.Length; supportIndex++)
            triangles.AddRange(trianglesBySupport[supportIndex]);

        return triangles;
    }

    private static void AppendSphere(List<Triangle3D> triangles, Vec3 centre, float radiusMm, int latSegments, int lonSegments)
    {
        for (var lat = 0; lat < latSegments; lat++)
        {
            var theta0 = MathF.PI * lat / latSegments;
            var theta1 = MathF.PI * (lat + 1) / latSegments;

            for (var lon = 0; lon < lonSegments; lon++)
            {
                var phi0 = MathF.PI * 2f * lon / lonSegments;
                var phi1 = MathF.PI * 2f * (lon + 1) / lonSegments;

                var p00 = SpherePoint(centre, radiusMm, theta0, phi0);
                var p01 = SpherePoint(centre, radiusMm, theta0, phi1);
                var p10 = SpherePoint(centre, radiusMm, theta1, phi0);
                var p11 = SpherePoint(centre, radiusMm, theta1, phi1);

                if (lat > 0)
                    triangles.Add(new Triangle3D(p00, p10, p01, ComputeNormal(p00, p10, p01)));

                if (lat < latSegments - 1)
                    triangles.Add(new Triangle3D(p01, p10, p11, ComputeNormal(p01, p10, p11)));
            }
        }
    }

    private static Vec3 SpherePoint(Vec3 centre, float radiusMm, float theta, float phi)
    {
        var sinTheta = MathF.Sin(theta);
        return new Vec3(
            centre.X + (radiusMm * sinTheta * MathF.Cos(phi)),
            centre.Y + (radiusMm * MathF.Cos(theta)),
            centre.Z + (radiusMm * sinTheta * MathF.Sin(phi)));
    }

    private static Vec3 ComputeNormal(Vec3 a, Vec3 b, Vec3 c)
        => (b - a).Cross(c - a).Normalized;

    private static LoadedGeometry CloneGeometry(LoadedGeometry geometry, List<Triangle3D> triangles) => new()
    {
        Triangles = triangles,
        DimensionXMm = geometry.DimensionXMm,
        DimensionYMm = geometry.DimensionYMm,
        DimensionZMm = geometry.DimensionZMm,
        SphereCentre = geometry.SphereCentre,
        SphereRadius = geometry.SphereRadius,
    };

    private AutoSupportTuning ResolveTuning(AutoSupportTuningOverrides? tuningOverrides)
    {
        if (tuningOverrides != null)
        {
            return new AutoSupportTuning(
                BedMarginMm: tuningOverrides.BedMarginMm,
                MinVoxelSizeMm: tuningOverrides.MinVoxelSizeMm,
                MaxVoxelSizeMm: tuningOverrides.MaxVoxelSizeMm,
                MinLayerHeightMm: tuningOverrides.MinLayerHeightMm,
                MaxLayerHeightMm: tuningOverrides.MaxLayerHeightMm,
                MinIslandAreaMm2: tuningOverrides.MinIslandAreaMm2,
                SupportSpacingThresholdMm: tuningOverrides.SupportSpacingThresholdMm,
                ResinStrength: tuningOverrides.ResinStrength,
                CrushForceThreshold: tuningOverrides.CrushForceThreshold,
                MaxAngularForce: tuningOverrides.MaxAngularForce,
                PeelForceMultiplier: tuningOverrides.PeelForceMultiplier,
                LightTipRadiusMm: tuningOverrides.LightTipRadiusMm,
                MediumTipRadiusMm: tuningOverrides.MediumTipRadiusMm,
                HeavyTipRadiusMm: tuningOverrides.HeavyTipRadiusMm,
                SuctionMultiplier: tuningOverrides.SuctionMultiplier,
                AreaGrowthThreshold: tuningOverrides.AreaGrowthThreshold,
                AreaGrowthMultiplier: tuningOverrides.AreaGrowthMultiplier,
                GravityEnabled: tuningOverrides.GravityEnabled,
                ResinDensityGPerMl: tuningOverrides.ResinDensityGPerMl,
                DragCoefficientMultiplier: tuningOverrides.DragCoefficientMultiplier,
                MinFeatureWidthMm: tuningOverrides.MinFeatureWidthMm,
                ShrinkagePercent: tuningOverrides.ShrinkagePercent,
                ShrinkageEdgeBias: tuningOverrides.ShrinkageEdgeBias,
                ModelLiftMm: tuningOverrides.ModelLiftMm,
                OverhangSensitivity: tuningOverrides.OverhangSensitivity,
                PeelDirection: tuningOverrides.PeelDirection,
                PeelStartMultiplier: tuningOverrides.PeelStartMultiplier,
                PeelEndMultiplier: tuningOverrides.PeelEndMultiplier,
                HeightBias: tuningOverrides.HeightBias,
                BridgeReductionFactor: tuningOverrides.BridgeReductionFactor,
                CantileverMomentMultiplier: tuningOverrides.CantileverMomentMultiplier,
                CantileverReferenceLengthMm: tuningOverrides.CantileverReferenceLengthMm,
                LayerBondStrengthPerMm2: tuningOverrides.LayerBondStrengthPerMm2,
                LayerAdhesionSafetyFactor: tuningOverrides.LayerAdhesionSafetyFactor,
                SupportInteractionEnabled: tuningOverrides.SupportInteractionEnabled,
                DrainageDepthForceMultiplier: tuningOverrides.DrainageDepthForceMultiplier,
                AccessibilityEnabled: tuningOverrides.AccessibilityEnabled,
                AccessibilityScanRadiusPx: tuningOverrides.AccessibilityScanRadiusPx,
                AccessibilityMinOpenDirections: tuningOverrides.AccessibilityMinOpenDirections,
                SurfaceQualityWeight: tuningOverrides.SurfaceQualityWeight,
                SurfaceQualitySearchRadiusPx: tuningOverrides.SurfaceQualitySearchRadiusPx,
                OrientationCheckEnabled: tuningOverrides.OrientationCheckEnabled,
                OrientationRiskForceMultiplierMax: tuningOverrides.OrientationRiskForceMultiplierMax,
                OrientationRiskThresholdRatio: tuningOverrides.OrientationRiskThresholdRatio);
        }

        var config = appConfigService?.GetAsync().GetAwaiter().GetResult();
        return new AutoSupportTuning(
            BedMarginMm: config?.AutoSupportBedMarginMm ?? AppConfigService.DefaultAutoSupportBedMarginMm,
            MinVoxelSizeMm: config?.AutoSupportMinVoxelSizeMm ?? AppConfigService.DefaultAutoSupportMinVoxelSizeMm,
            MaxVoxelSizeMm: config?.AutoSupportMaxVoxelSizeMm ?? AppConfigService.DefaultAutoSupportMaxVoxelSizeMm,
            MinLayerHeightMm: config?.AutoSupportMinLayerHeightMm ?? AppConfigService.DefaultAutoSupportMinLayerHeightMm,
            MaxLayerHeightMm: config?.AutoSupportMaxLayerHeightMm ?? AppConfigService.DefaultAutoSupportMaxLayerHeightMm,
            MinIslandAreaMm2: config?.AutoSupportMinIslandAreaMm2 ?? AppConfigService.DefaultAutoSupportMinIslandAreaMm2,
            SupportSpacingThresholdMm: config?.AutoSupportMergeDistanceMm ?? 3f,
            ResinStrength: config?.AutoSupportResinStrength ?? AppConfigService.DefaultAutoSupportResinStrength,
            CrushForceThreshold: config?.AutoSupportCrushForceThreshold ?? AppConfigService.DefaultAutoSupportCrushForceThreshold,
            MaxAngularForce: config?.AutoSupportMaxAngularForce ?? AppConfigService.DefaultAutoSupportMaxAngularForce,
            PeelForceMultiplier: config?.AutoSupportPeelForceMultiplier ?? AppConfigService.DefaultAutoSupportPeelForceMultiplier,
            LightTipRadiusMm: config?.AutoSupportLightTipRadiusMm ?? AppConfigService.DefaultAutoSupportLightTipRadiusMm,
            MediumTipRadiusMm: config?.AutoSupportMediumTipRadiusMm ?? AppConfigService.DefaultAutoSupportMediumTipRadiusMm,
            HeavyTipRadiusMm: config?.AutoSupportHeavyTipRadiusMm ?? AppConfigService.DefaultAutoSupportHeavyTipRadiusMm,
            SuctionMultiplier: config?.AutoSupportSuctionMultiplier ?? AppConfigService.DefaultAutoSupportSuctionMultiplier,
            AreaGrowthThreshold: config?.AutoSupportAreaGrowthThreshold ?? AppConfigService.DefaultAutoSupportAreaGrowthThreshold,
            AreaGrowthMultiplier: config?.AutoSupportAreaGrowthMultiplier ?? AppConfigService.DefaultAutoSupportAreaGrowthMultiplier,
            GravityEnabled: config?.AutoSupportGravityEnabled ?? AppConfigService.DefaultAutoSupportGravityEnabled,
            ResinDensityGPerMl: config?.AutoSupportResinDensityGPerMl ?? AppConfigService.DefaultAutoSupportResinDensityGPerMl,
            DragCoefficientMultiplier: config?.AutoSupportDragCoefficientMultiplier ?? AppConfigService.DefaultAutoSupportDragCoefficientMultiplier,
            MinFeatureWidthMm: config?.AutoSupportMinFeatureWidthMm ?? AppConfigService.DefaultAutoSupportMinFeatureWidthMm,
            ShrinkagePercent: config?.AutoSupportShrinkagePercent ?? AppConfigService.DefaultAutoSupportShrinkagePercent,
            ShrinkageEdgeBias: config?.AutoSupportShrinkageEdgeBias ?? AppConfigService.DefaultAutoSupportShrinkageEdgeBias,
                ModelLiftMm: config?.AutoSupportModelLiftMm ?? AppConfigService.DefaultAutoSupportModelLiftMm,
                OverhangSensitivity: config?.AutoSupportOverhangSensitivity ?? AppConfigService.DefaultAutoSupportOverhangSensitivity,
                PeelDirection: Enum.IsDefined(typeof(PeelDirection), config?.AutoSupportPeelDirection ?? AppConfigService.DefaultAutoSupportPeelDirection)
                    ? (PeelDirection)(config?.AutoSupportPeelDirection ?? AppConfigService.DefaultAutoSupportPeelDirection)
                    : PeelDirection.ZPositive,
                PeelStartMultiplier: config?.AutoSupportPeelStartMultiplier ?? AppConfigService.DefaultAutoSupportPeelStartMultiplier,
                PeelEndMultiplier: config?.AutoSupportPeelEndMultiplier ?? AppConfigService.DefaultAutoSupportPeelEndMultiplier,
                HeightBias: config?.AutoSupportHeightBias ?? AppConfigService.DefaultAutoSupportHeightBias,
                BridgeReductionFactor: config?.AutoSupportBridgeReductionFactor ?? AppConfigService.DefaultAutoSupportBridgeReductionFactor,
                CantileverMomentMultiplier: config?.AutoSupportCantileverMomentMultiplier ?? AppConfigService.DefaultAutoSupportCantileverMomentMultiplier,
                CantileverReferenceLengthMm: config?.AutoSupportCantileverReferenceLengthMm ?? AppConfigService.DefaultAutoSupportCantileverReferenceLengthMm,
                LayerBondStrengthPerMm2: config?.AutoSupportLayerBondStrengthPerMm2 ?? AppConfigService.DefaultAutoSupportLayerBondStrengthPerMm2,
                LayerAdhesionSafetyFactor: config?.AutoSupportLayerAdhesionSafetyFactor ?? AppConfigService.DefaultAutoSupportLayerAdhesionSafetyFactor,
                SupportInteractionEnabled: config?.AutoSupportSupportInteractionEnabled ?? AppConfigService.DefaultAutoSupportSupportInteractionEnabled,
                DrainageDepthForceMultiplier: config?.AutoSupportDrainageDepthForceMultiplier ?? AppConfigService.DefaultAutoSupportDrainageDepthForceMultiplier,
                AccessibilityEnabled: config?.AutoSupportAccessibilityEnabled ?? AppConfigService.DefaultAutoSupportAccessibilityEnabled,
                AccessibilityScanRadiusPx: config?.AutoSupportAccessibilityScanRadiusPx ?? AppConfigService.DefaultAutoSupportAccessibilityScanRadiusPx,
                AccessibilityMinOpenDirections: config?.AutoSupportAccessibilityMinOpenDirections ?? AppConfigService.DefaultAutoSupportAccessibilityMinOpenDirections,
                SurfaceQualityWeight: config?.AutoSupportSurfaceQualityWeight ?? AppConfigService.DefaultAutoSupportSurfaceQualityWeight,
                SurfaceQualitySearchRadiusPx: config?.AutoSupportSurfaceQualitySearchRadiusPx ?? AppConfigService.DefaultAutoSupportSurfaceQualitySearchRadiusPx,
                OrientationCheckEnabled: config?.AutoSupportOrientationCheckEnabled ?? AppConfigService.DefaultAutoSupportOrientationCheckEnabled,
                OrientationRiskForceMultiplierMax: config?.AutoSupportOrientationRiskForceMultiplierMax ?? AppConfigService.DefaultAutoSupportOrientationRiskForceMultiplierMax,
                OrientationRiskThresholdRatio: config?.AutoSupportOrientationRiskThresholdRatio ?? AppConfigService.DefaultAutoSupportOrientationRiskThresholdRatio);
    }

    private sealed record AutoSupportTuning(
        float BedMarginMm,
        float MinVoxelSizeMm,
        float MaxVoxelSizeMm,
        float MinLayerHeightMm,
        float MaxLayerHeightMm,
        float MinIslandAreaMm2,
        float SupportSpacingThresholdMm,
        float ResinStrength,
        float CrushForceThreshold,
        float MaxAngularForce,
        float PeelForceMultiplier,
        float LightTipRadiusMm,
        float MediumTipRadiusMm,
        float HeavyTipRadiusMm,
        float SuctionMultiplier,
        float AreaGrowthThreshold,
        float AreaGrowthMultiplier,
        bool GravityEnabled,
        float ResinDensityGPerMl,
        float DragCoefficientMultiplier,
        float MinFeatureWidthMm,
        float ShrinkagePercent,
        float ShrinkageEdgeBias,
        float ModelLiftMm,
        float OverhangSensitivity,
        PeelDirection PeelDirection,
        float PeelStartMultiplier,
        float PeelEndMultiplier,
        float HeightBias,
        float BridgeReductionFactor,
        float CantileverMomentMultiplier,
        float CantileverReferenceLengthMm,
        float LayerBondStrengthPerMm2,
        float LayerAdhesionSafetyFactor,
        bool SupportInteractionEnabled,
        float DrainageDepthForceMultiplier,
        bool AccessibilityEnabled,
        int AccessibilityScanRadiusPx,
        int AccessibilityMinOpenDirections,
        float SurfaceQualityWeight,
        int SurfaceQualitySearchRadiusPx,
        bool OrientationCheckEnabled,
        float OrientationRiskForceMultiplierMax,
        float OrientationRiskThresholdRatio);

    private static float GetTipRadiusMm(SupportSize size, AutoSupportTuning tuning) => size switch
    {
        SupportSize.Heavy => tuning.HeavyTipRadiusMm,
        SupportSize.Medium => tuning.MediumTipRadiusMm,
        _ => tuning.LightTipRadiusMm,
    };

    // Item 1: Detect enclosed (suction-prone) pixels via flood-fill from bitmap edges.
    // Returns the set of pixels that are NOT reachable from the bitmap boundary through
    // empty space - meaning they are enclosed by cured geometry and subject to suction.
    private static HashSet<(int X, int Y)>? DetectEnclosedPixels(SliceBitmap bitmap)
    {
        var width = bitmap.Width;
        var height = bitmap.Height;
        var reachable = new bool[bitmap.Pixels.Length];
        var queue = new Queue<(int X, int Y)>();

        // Seed with all empty edge pixels
        for (var x = 0; x < width; x++)
        {
            if (bitmap.Pixels[x] == 0 && !reachable[x])
            {
                reachable[x] = true;
                queue.Enqueue((x, 0));
            }

            var bottomIdx = ((height - 1) * width) + x;
            if (bitmap.Pixels[bottomIdx] == 0 && !reachable[bottomIdx])
            {
                reachable[bottomIdx] = true;
                queue.Enqueue((x, height - 1));
            }
        }

        for (var y = 0; y < height; y++)
        {
            var leftIdx = y * width;
            if (bitmap.Pixels[leftIdx] == 0 && !reachable[leftIdx])
            {
                reachable[leftIdx] = true;
                queue.Enqueue((0, y));
            }

            var rightIdx = (y * width) + width - 1;
            if (bitmap.Pixels[rightIdx] == 0 && !reachable[rightIdx])
            {
                reachable[rightIdx] = true;
                queue.Enqueue((width - 1, y));
            }
        }

        // BFS flood-fill through empty pixels
        while (queue.Count > 0)
        {
            var (cx, cy) = queue.Dequeue();
            foreach (var (nx, ny) in Neighbors(cx, cy))
            {
                if (nx < 0 || ny < 0 || nx >= width || ny >= height)
                    continue;
                var ni = (ny * width) + nx;
                if (reachable[ni] || bitmap.Pixels[ni] != 0)
                    continue;
                reachable[ni] = true;
                queue.Enqueue((nx, ny));
            }
        }

        // Any cured pixel whose interior neighbor empty pixels are unreachable is enclosed
        // We look for cured pixels adjacent to unreachable empty pixels (enclosed voids)
        HashSet<(int X, int Y)>? enclosed = null;
        for (var y = 0; y < height; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var idx = (y * width) + x;
                if (bitmap.Pixels[idx] == 0)
                    continue;

                // Check if this cured pixel borders any unreachable empty pixel
                var bordersEnclosure = false;
                foreach (var (nx, ny) in Neighbors(x, y))
                {
                    if (nx < 0 || ny < 0 || nx >= width || ny >= height)
                        continue;
                    var ni = (ny * width) + nx;
                    if (bitmap.Pixels[ni] == 0 && !reachable[ni])
                    {
                        bordersEnclosure = true;
                        break;
                    }
                }

                if (bordersEnclosure)
                {
                    enclosed ??= new HashSet<(int X, int Y)>();
                    enclosed.Add((x, y));
                }
            }
        }

        return enclosed;
    }

    // Item 3: Apply cumulative gravitational loading per support.
    // For each support, accumulate the weight of connected geometry above it
    // and upgrade supports that are overloaded by gravitational mass.
    private static bool ApplyGravityLoading(
        List<TipIsland>[] layerIslands,
        HashSet<(int Column, int Row)>?[] prevLayerPixels,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        AutoSupportTuning tuning,
        out int upgradedCount)
    {
        upgradedCount = 0;
        var cumulativeWeightPerSupport = ComputeCumulativeWeightPerSupport(
            layerIslands,
            prevLayerPixels,
            supportPoints,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm,
            tuning);

        var changedFlag = 0;
        var upgrades = 0;
        var newSizes = new SupportSize[supportPoints.Count];

        Parallel.For(0, supportPoints.Count, FullCoreParallelOptions, i =>
        {
            var support = supportPoints[i];
            var newSize = support.Size;
            var capacity = ComputeCapacity(support, tuning);
            var gravityLoad = cumulativeWeightPerSupport[i];

            if (gravityLoad > capacity * 0.5f)
            {
                if (gravityLoad > capacity * 1.5f)
                    newSize = SupportSize.Heavy;
                else if (gravityLoad > capacity)
                    newSize = support.Size == SupportSize.Light ? SupportSize.Medium : support.Size;
            }

            newSizes[i] = newSize;

            if (newSize != support.Size)
            {
                Interlocked.Exchange(ref changedFlag, 1);
                Interlocked.Increment(ref upgrades);
            }
        });

        for (var i = 0; i < supportPoints.Count; i++)
        {
            var support = supportPoints[i];
            var newSize = newSizes[i];
            if (newSize == support.Size)
                continue;
            supportPoints[i] = support with { Size = newSize, RadiusMm = GetTipRadiusMm(newSize, tuning) };
        }

        upgradedCount = upgrades;

        return Volatile.Read(ref changedFlag) == 1;
    }

    private static float[] ComputeCumulativeWeightPerSupport(
        List<TipIsland>[] layerIslands,
        HashSet<(int Column, int Row)>?[] prevLayerPixels,
        List<SupportPoint> supportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        AutoSupportTuning tuning)
    {
        var pixelAreaMm2 = (bedWidthMm / pixelWidth) * (bedDepthMm / pixelHeight);
        var voxelVolumeMm3 = pixelAreaMm2 * layerHeightMm;
        var gravitationalAcceleration = 9.81f;
        var cumulativeWeightPerSupport = new float[supportPoints.Count];
        if (supportPoints.Count == 0)
            return cumulativeWeightPerSupport;

        var mergeLock = new object();

        // Bottom-up pass: assign each layer's volume to its supporting supports.
        Parallel.ForEach(
            Partitioner.Create(0, layerIslands.Length),
            FullCoreParallelOptions,
            () => new float[supportPoints.Count],
            (range, _, localWeights) =>
            {
                for (var layerIndex = range.Item1; layerIndex < range.Item2; layerIndex++)
                {
                    var prevPixels = prevLayerPixels[layerIndex];
                    foreach (var island in layerIslands[layerIndex])
                    {
                        var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                        if (unsupportedPixels.Count == 0)
                            continue;
                        var islandPixelSet = island.PixelCoords.ToHashSet();

                        var supportIndices = FindSupportingSupportsForPixelSet(
                            islandPixelSet,
                            island.SliceHeightMm,
                            supportPoints,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            layerHeightMm);

                        if (supportIndices.Count == 0)
                            continue;

                        var layerMassG = island.PixelCoords.Count * voxelVolumeMm3 * (tuning.ResinDensityGPerMl / 1000f);
                        var layerWeightN = layerMassG * gravitationalAcceleration;
                        var perSupportWeight = layerWeightN / supportIndices.Count;

                        foreach (var idx in supportIndices)
                        {
                            if (idx < localWeights.Length)
                                localWeights[idx] += perSupportWeight;
                        }
                    }
                }

                return localWeights;
            },
            localWeights =>
            {
                lock (mergeLock)
                {
                    for (var i = 0; i < localWeights.Length; i++)
                        cumulativeWeightPerSupport[i] += localWeights[i];
                }
            });

        return cumulativeWeightPerSupport;
    }

    // Item 5: Place additional supports at edges of large flat islands to counter shrinkage curling.
    private static bool ApplyShrinkageEdgeSupports(
        List<TipIsland>[] layerIslands,
        HashSet<(int Column, int Row)>?[] layerPixelSets,
        IReadOnlyDictionary<(int Column, int Row), int> lowestLayerByPixel,
        HashSet<(int Column, int Row)>?[] prevLayerPixels,
        List<SupportPoint> supportPoints,
        int maxSupportPoints,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        AutoSupportTuning tuning)
    {
        var shrinkageFactor = tuning.ShrinkagePercent / 100f;
        var pixelWidthMm = bedWidthMm / pixelWidth;
        var pixelDepthMm = bedDepthMm / pixelHeight;
        var changed = false;

        // Threshold: large islands with low perimeter-to-area ratio are shrinkage-prone
        var minAreaForShrinkage = 25f; // mm2, only large flat areas need edge supports

        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            if (supportPoints.Count >= maxSupportPoints)
                return changed;

            var prevPixels = prevLayerPixels[layerIndex];
            foreach (var island in layerIslands[layerIndex])
            {
                if (supportPoints.Count >= maxSupportPoints)
                    return changed;

                var isTipIsland = IsTipIsland(island.PixelCoords, prevPixels);
                if (!isTipIsland && layerIndex < Math.Max(1, layerIslands.Length / 4))
                    continue;

                var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                if (unsupportedPixels.Count == 0)
                    continue;

                if (island.TopLayerAreaMm2 < minAreaForShrinkage)
                    continue;

                // Low perimeter-to-area ratio means more bulk, more shrinkage stress
                if (island.PerimeterToAreaRatio > 1.0f)
                    continue;

                // Find edge pixels (those with at least one empty neighbor)
                var edgePixels = new List<(int X, int Y)>();
                var pixelSet = new HashSet<(int X, int Y)>(unsupportedPixels);
                foreach (var (x, y) in unsupportedPixels)
                {
                    foreach (var (nx, ny) in Neighbors(x, y))
                    {
                        if (!pixelSet.Contains((nx, ny)))
                        {
                            edgePixels.Add((x, y));
                            break;
                        }
                    }
                }

                if (edgePixels.Count == 0)
                    continue;

                // Check existing support coverage at edges
                var existingSupportsAtEdge = 0;
                foreach (var support in supportPoints)
                {
                    if (MathF.Abs(support.Position.Y - island.SliceHeightMm) > pixelDepthMm * 2f)
                        continue;

                    var col = (int)MathF.Floor(((support.Position.X + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth);
                    var row = (int)MathF.Floor((((bedDepthMm * 0.5f) - support.Position.Z) / bedDepthMm) * pixelHeight);

                    foreach (var (ex, ey) in edgePixels)
                    {
                        if (Math.Abs(col - ex) <= 2 && Math.Abs(row - ey) <= 2)
                        {
                            existingSupportsAtEdge++;
                            break;
                        }
                    }
                }

                // Determine how many edge supports we need based on shrinkage and area
                var shrinkageStress = shrinkageFactor * island.TopLayerAreaMm2 * tuning.ShrinkageEdgeBias;
                var desiredEdgeSupports = Math.Max(1, (int)(shrinkageStress / 10f));
                var edgeSupportsToAdd = Math.Max(0, desiredEdgeSupports - existingSupportsAtEdge);

                if (edgeSupportsToAdd <= 0)
                    continue;

                // Select edge pixels spread out for placement
                var step = Math.Max(1, edgePixels.Count / edgeSupportsToAdd);
                for (var e = 0; e < edgeSupportsToAdd && e * step < edgePixels.Count; e++)
                {
                    var (ex, ey) = edgePixels[e * step];
                    var xMm = ColumnToX(ex, bedWidthMm, pixelWidth);
                    var zMm = RowToZ(ey, bedDepthMm, pixelHeight);
                    var shrinkageAddResult = TryAddSupportAtBestLayer(
                        supportPoints,
                        maxSupportPoints,
                        edgePixels,
                        layerPixelSets,
                        lowestLayerByPixel,
                        layerIndex,
                        xMm,
                        zMm,
                        SupportSize.Light,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm,
                        tuning.SupportSpacingThresholdMm,
                        tuning);
                    if (shrinkageAddResult == SupportPlacementResult.CapReached)
                        return changed;
                    if (shrinkageAddResult == SupportPlacementResult.NoValidPlacement)
                        break;
                    changed = true;
                }
            }
        }

        return changed;
    }

    private sealed record TipIsland(
        List<(int X, int Y)> PixelCoords,
        float CentroidX,
        float CentroidZ,
        float SliceHeightMm,
        float TopLayerAreaMm2,
        bool HasEnclosedRegion = false,
        float EnclosureRatio = 0f,
        int EnclosureDepthLayers = 1,
        float AspectRatio = 1f,
        float MinWidthMm = float.MaxValue,
        float PerimeterToAreaRatio = 0f);

    private sealed record VoxelRect(int X, int Y, int Width, int Height);

    private sealed record VoxelRectKey(int X, int Y, int Width, int Height);

    private sealed record FurthestPointCandidate(float XMm, float ZMm, float DistanceMm);

    private sealed record PixelCentroid(float XMm, float ZMm);

    private sealed class MutableLayerForce(int layerIndex, float sliceHeightMm)
    {
        public int LayerIndex { get; } = layerIndex;
        public float SliceHeightMm { get; } = sliceHeightMm;
        public Vec3 Gravity { get; set; } = new(0f, 0f, 0f);
        public Vec3 Peel { get; set; } = new(0f, 0f, 0f);
        public Vec3 Rotation { get; set; } = new(0f, 0f, 0f);
    }

    private sealed class SupportLayerForceAccumulator
    {
        public SupportLayerForceAccumulator(int supportCount)
        {
            AccumulatedForces = new Vec3[supportCount];
            LayerForcesBySupport = new Dictionary<int, MutableLayerForce>[supportCount];
            for (var i = 0; i < supportCount; i++)
                LayerForcesBySupport[i] = new Dictionary<int, MutableLayerForce>();
        }

        public Vec3[] AccumulatedForces { get; }

        public Dictionary<int, MutableLayerForce>[] LayerForcesBySupport { get; }
    }

    private enum SupportPlacementResult
    {
        Added,
        NoValidPlacement,
        CapReached,
    }

    private sealed record SupportForceMetric(
        int SupportIndex,
        Vec3 PullVector,
        Vec3 PeelVector,
        Vec3 RotationVector,
        float VerticalPullForce,
        float CompressiveForce,
        float AngularForce);

    private sealed record IslandTopologyMetrics(
        IslandTopology Topology,
        float AnchoredPerimeterRatio,
        float CantileverLengthMm);

    private sealed record SupportInteractionMetrics(
        float CapacityMultiplier,
        float LoadMultiplier,
        bool RequiresRedistribution);

    private enum IslandTopology
    {
        Floating,
        Cantilever,
        Bridge,
    }
}
