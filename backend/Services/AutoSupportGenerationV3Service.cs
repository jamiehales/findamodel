using Microsoft.Extensions.Logging;
using System.Diagnostics;
using System.Collections.Concurrent;
using System.Threading;

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
public sealed class AutoSupportGenerationV3Service
{
    private const int TrianglesPerVoxelBlock = 12;
    private const int MaxPreviewVoxelTriangles = 2_000_000;
    private const int MaxReinforcementIterationsPerIsland = 32;
    private const int MaxSolverPasses = 8;
    private static readonly ParallelOptions FullCoreParallelOptions = new()
    {
        MaxDegreeOfParallelism = Math.Max(1, Environment.ProcessorCount),
    };

    private readonly ILogger logger;
    private readonly OrthographicProjectionSliceBitmapGenerator slicer = new();
    private readonly AppConfigService? appConfigService;

    public AutoSupportGenerationV3Service(ILoggerFactory loggerFactory)
        : this(null, loggerFactory)
    {
    }

    public AutoSupportGenerationV3Service(AppConfigService? appConfigService, ILoggerFactory loggerFactory)
    {
        this.appConfigService = appConfigService;
        logger = loggerFactory.CreateLogger<AutoSupportGenerationV3Service>();
    }

    public SupportPreviewResult GenerateSupportPreview(LoadedGeometry geometry, AutoSupportV3TuningOverrides? tuningOverrides = null)
    {
        ArgumentNullException.ThrowIfNull(geometry);

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
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight,
                layerHeightMm,
                layerAreaGrowthRatio,
                tuning);

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

            if (!changed)
                break;
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
            tuning);

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

        // Islands list is intentionally empty - only the support spheres and voxel body are returned for visualisation
        return new SupportPreviewResult(supportPoints, CloneGeometry(geometry, supportTriangles), [], voxelGeometry);
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
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        float[] layerAreaGrowthRatio,
        V3Tuning tuning)
    {
        var changed = false;

        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            var prevPixels = prevLayerPixels[layerIndex];
            foreach (var island in layerIslands[layerIndex])
            {
                var islandPixelSet = island.PixelCoords.ToHashSet();
                var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                if (unsupportedPixels.Count == 0)
                    continue;

                for (var i = 0; i < MaxReinforcementIterationsPerIsland; i++)
                {
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
                    {
                        var centroid = ComputePixelCentroidMm(unsupportedPixels, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight);
                        AddSupportAtBestLayer(
                            supportPoints,
                            island.PixelCoords,
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
                            tuning);
                        changed = true;
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
                        layerAreaGrowthRatio[layerIndex]);

                    var combinedCapacity = supportIndices.Sum(index => ComputeCapacity(supportPoints[index], tuning));
                    var verticalPullForce = supportForces.Sum(metric => metric.VerticalPullForce);
                    var maxCompressiveForce = supportForces.Max(metric => metric.CompressiveForce);
                    var maxAngularForce = supportForces.Max(metric => metric.AngularForce);
                    var isOverloaded = verticalPullForce > combinedCapacity;
                    var exceedsCrushForce = maxCompressiveForce > tuning.CrushForceThreshold;
                    var exceedsAngularForce = maxAngularForce > tuning.MaxAngularForce;
                    var exceedsSpacing = furthest.DistanceMm > tuning.SupportSpacingThresholdMm;
                    var strictSupportIndices = FindSupportingSupportsForPixelSet(
                        islandPixelSet,
                        island.SliceHeightMm,
                        supportPoints,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm,
                        includeNeighborPixels: false);
                    var islandWouldLoseSupportOnMerge = strictSupportIndices.Count == 0;

                    if (islandWouldLoseSupportOnMerge)
                    {
                        var centroid = ComputePixelCentroidMm(
                            unsupportedPixels,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight);
                        AddSupportAtBestLayer(
                            supportPoints,
                            island.PixelCoords,
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
                            tuning);
                        changed = true;
                        continue;
                    }

                    if (!isOverloaded && !exceedsCrushForce && !exceedsAngularForce && !exceedsSpacing)
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

                    AddSupportAtBestLayer(
                        supportPoints,
                        island.PixelCoords,
                        layerPixelSets,
                        lowestLayerByPixel,
                        layerIndex,
                        furthest.XMm,
                        furthest.ZMm,
                        newSize,
                        bedWidthMm,
                        bedDepthMm,
                        pixelWidth,
                        pixelHeight,
                        layerHeightMm,
                        tuning);
                    changed = true;
                }
            }
        }

        return changed;
    }

    private static void AddSupportAtBestLayer(
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
        V3Tuning tuning)
    {
        if (candidatePixels.Count == 0)
            return;

        var bestPixel = candidatePixels[0];
        var bestLayer = currentLayerIndex;
        var bestDistSq = float.MaxValue;

        foreach (var pixel in candidatePixels)
        {
            var candidateLayer = lowestLayerByPixel.TryGetValue(pixel, out var cachedLayer)
                ? Math.Min(cachedLayer, currentLayerIndex)
                : currentLayerIndex;
            var xMm = ColumnToX(pixel.X, bedWidthMm, pixelWidth);
            var zMm = RowToZ(pixel.Y, bedDepthMm, pixelHeight);
            var dx = xMm - targetXMm;
            var dz = zMm - targetZMm;
            var distSq = (dx * dx) + (dz * dz);

            if (candidateLayer < bestLayer || (candidateLayer == bestLayer && distSq < bestDistSq))
            {
                bestLayer = candidateLayer;
                bestPixel = pixel;
                bestDistSq = distSq;
            }
        }

        var yMm = (bestLayer * layerHeightMm) + (layerHeightMm * 0.5f);
        var xPlacementMm = ColumnToX(bestPixel.X, bedWidthMm, pixelWidth);
        var zPlacementMm = RowToZ(bestPixel.Y, bedDepthMm, pixelHeight);

        supportPoints.Add(new SupportPoint(
            new Vec3(xPlacementMm, yMm, zPlacementMm),
            GetTipRadiusMm(size, tuning),
            new Vec3(0f, 0f, 0f),
            size));
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
        V3Tuning tuning)
    {
        if (supportPoints.Count == 0)
            return;

        var accumulatedForces = new Vec3[supportPoints.Count];
        var mergeLock = new object();

        Parallel.ForEach(
            Partitioner.Create(0, layerIslands.Length),
            FullCoreParallelOptions,
            () => new Vec3[supportPoints.Count],
            (range, _, localForces) =>
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

                        var supportForces = EvaluateSupportForces(
                            unsupportedPixels,
                            supportIndices,
                            supportPoints,
                            bedWidthMm,
                            bedDepthMm,
                            pixelWidth,
                            pixelHeight,
                            tuning,
                            island);

                        foreach (var metric in supportForces)
                            localForces[metric.SupportIndex] = localForces[metric.SupportIndex] + metric.PullVector;
                    }
                }

                return localForces;
            },
            localForces =>
            {
                lock (mergeLock)
                {
                    for (var i = 0; i < localForces.Length; i++)
                        accumulatedForces[i] = accumulatedForces[i] + localForces[i];
                }
            });

        for (var i = 0; i < supportPoints.Count; i++)
            supportPoints[i] = supportPoints[i] with { PullForce = accumulatedForces[i] };
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
        V3Tuning tuning,
        TipIsland? island = null,
        float areaGrowthRatio = 0f)
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

        var effectivePixelForce = pixelForce * suctionMultiplier * areaGrowthMultiplier;
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
                metrics.Add(new SupportForceMetric(supportIndex, new Vec3(0f, 0f, 0f), 0f, 0f, 0f));
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

            metrics.Add(new SupportForceMetric(
                supportIndex,
                new Vec3(lateralX, signedVerticalForce, lateralZ),
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
        bool includeNeighborPixels = true)
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
            includeNeighborPixels);
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
        bool includeNeighborPixels = true)
    {
        var indices = new List<int>();
        var maxY = sliceHeightMm + (layerHeightMm * 0.5f);

        var shouldParallelize = supportPoints.Count >= 1024;
        if (!shouldParallelize)
        {
            for (var i = 0; i < supportPoints.Count; i++)
            {
                var support = supportPoints[i];
                if (support.Position.Y > maxY)
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
                if (support.Position.Y > maxY)
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

    private static float ComputeCapacity(SupportPoint point, V3Tuning tuning)
        => MathF.PI * point.RadiusMm * point.RadiusMm * tuning.ResinStrength;

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

    private V3Tuning ResolveTuning(AutoSupportV3TuningOverrides? tuningOverrides)
    {
        if (tuningOverrides != null)
        {
            return new V3Tuning(
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
                ShrinkageEdgeBias: tuningOverrides.ShrinkageEdgeBias);
        }

        var config = appConfigService?.GetAsync().GetAwaiter().GetResult();
        return new V3Tuning(
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
            ShrinkageEdgeBias: config?.AutoSupportShrinkageEdgeBias ?? AppConfigService.DefaultAutoSupportShrinkageEdgeBias);
    }

    private sealed record V3Tuning(
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
        float ShrinkageEdgeBias);

    private static float GetTipRadiusMm(SupportSize size, V3Tuning tuning) => size switch
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
        V3Tuning tuning,
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
        V3Tuning tuning)
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
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerHeightMm,
        V3Tuning tuning)
    {
        var shrinkageFactor = tuning.ShrinkagePercent / 100f;
        var pixelWidthMm = bedWidthMm / pixelWidth;
        var pixelDepthMm = bedDepthMm / pixelHeight;
        var changed = false;

        // Threshold: large islands with low perimeter-to-area ratio are shrinkage-prone
        var minAreaForShrinkage = 25f; // mm2, only large flat areas need edge supports

        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            var prevPixels = prevLayerPixels[layerIndex];
            foreach (var island in layerIslands[layerIndex])
            {
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
                    AddSupportAtBestLayer(
                        supportPoints,
                        island.PixelCoords,
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
                        tuning);
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
        float AspectRatio = 1f,
        float MinWidthMm = float.MaxValue,
        float PerimeterToAreaRatio = 0f);

    private sealed record VoxelRect(int X, int Y, int Width, int Height);

    private sealed record VoxelRectKey(int X, int Y, int Width, int Height);

    private sealed record FurthestPointCandidate(float XMm, float ZMm, float DistanceMm);

    private sealed record PixelCentroid(float XMm, float ZMm);

    private sealed class SupportForceAccumulator
    {
        public int PixelCount { get; set; }
        public float SumX { get; set; }
        public float SumZ { get; set; }
        public float AngularForce { get; set; }
    }

    private sealed record SupportForceMetric(
        int SupportIndex,
        Vec3 PullVector,
        float VerticalPullForce,
        float CompressiveForce,
        float AngularForce);
}
