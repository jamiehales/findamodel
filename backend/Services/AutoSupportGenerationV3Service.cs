using Microsoft.Extensions.Logging;

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

    public SupportPreviewResult GenerateSupportPreview(LoadedGeometry geometry)
    {
        ArgumentNullException.ThrowIfNull(geometry);

        if (geometry.Triangles.Count == 0)
            return new SupportPreviewResult([], CloneGeometry(geometry, []), []);

        var tuning = ResolveTuning();
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

        // Render all layers in parallel, collect 2-D islands and 2-D voxel rectangles per layer.
        // Each layer is independent so Parallel.For is safe here.
        var layerIslands = new List<TipIsland>[layerCount];
        var layerVoxelRects = new List<VoxelRect>[layerCount];
        Parallel.For(0, layerCount, layerIndex =>
        {
            var sliceHeightMm = (layerIndex * layerHeightMm) + (layerHeightMm * 0.5f);
            var bitmap = slicer.RenderLayerBitmap(
                geometry.Triangles,
                sliceHeightMm,
                bedWidthMm,
                bedDepthMm,
                pixelWidth,
                pixelHeight,
                layerHeightMm);

            layerIslands[layerIndex] = FindTipIslands(bitmap, bedWidthMm, bedDepthMm, sliceHeightMm, tuning.MinIslandAreaMm2);
            layerVoxelRects[layerIndex] = ExtractVoxelRects(bitmap);
        });

        // An island is an overhang "tip" when none of its pixels exist in the layer below -
        // i.e. it is the first (lowest) appearance of that connected region, requiring a support.
        // Build per-layer pixel sets for the layer below upfront so the check is a simple hash lookup.
        var prevLayerPixels = new HashSet<(int Column, int Row)>?[layerCount];
        for (var layerIndex = 1; layerIndex < layerCount; layerIndex++)
        {
            var below = layerIslands[layerIndex - 1];
            if (below.Count == 0)
                continue;

            var set = new HashSet<(int Column, int Row)>();
            foreach (var island in below)
                foreach (var pixel in island.PixelCoords)
                    set.Add(pixel);
            prevLayerPixels[layerIndex] = set;
        }

        var supportPoints = new List<SupportPoint>();

        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            var prevPixels = prevLayerPixels[layerIndex];

            foreach (var island in layerIslands[layerIndex])
            {
                var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                if (unsupportedPixels.Count == 0)
                    continue;

                var centroid = ComputePixelCentroidMm(unsupportedPixels, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight);

                supportPoints.Add(new SupportPoint(
                    new Vec3(centroid.XMm, island.SliceHeightMm, centroid.ZMm),
                    GetTipRadiusMm(SupportSize.Light, tuning),
                    new Vec3(0f, 0f, 0f),
                    SupportSize.Light));
            }
        }

        ReinforceSupportsByForceAndSpacing(
            layerIslands,
            prevLayerPixels,
            supportPoints,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight,
            layerHeightMm,
            tuning);

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

                visited[index] = true;
                queue.Enqueue((column, row));

                while (queue.Count > 0)
                {
                    var (cx, cy) = queue.Dequeue();
                    pixelCoords.Add((cx, cy));
                    sumX += ColumnToX(cx, bedWidthMm, bitmap.Width);
                    sumZ += RowToZ(cy, bedDepthMm, bitmap.Height);

                    foreach (var (nx, ny) in Neighbors(cx, cy))
                    {
                        if (nx < 0 || ny < 0 || nx >= bitmap.Width || ny >= bitmap.Height)
                            continue;
                        var ni = (ny * bitmap.Width) + nx;
                        if (visited[ni] || bitmap.Pixels[ni] == 0)
                            continue;
                        visited[ni] = true;
                        queue.Enqueue((nx, ny));
                    }
                }

                if (pixelCoords.Count * pixelAreaMm2 < minIslandAreaMm2)
                    continue;

                islands.Add(new TipIsland(
                    pixelCoords,
                    sumX / pixelCoords.Count,
                    sumZ / pixelCoords.Count,
                    sliceHeightMm,
                    pixelCoords.Count * pixelAreaMm2));
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

    private static void ReinforceSupportsByForceAndSpacing(
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
        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            var prevPixels = prevLayerPixels[layerIndex];
            foreach (var island in layerIslands[layerIndex])
            {
                var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                if (unsupportedPixels.Count == 0)
                    continue;

                var maxIterations = Math.Max(1, tuning.MaxSupportsPerIsland);
                for (var i = 0; i < maxIterations; i++)
                {
                    var supportIndices = FindSupportingSupportsForPixels(
                        unsupportedPixels,
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
                        supportPoints.Add(new SupportPoint(
                            new Vec3(centroid.XMm, island.SliceHeightMm, centroid.ZMm),
                            GetTipRadiusMm(SupportSize.Light, tuning),
                            new Vec3(0f, 0f, 0f),
                            SupportSize.Light));
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
                        tuning);

                    var combinedCapacity = supportIndices.Sum(index => ComputeCapacity(supportPoints[index], tuning));
                    var verticalPullForce = supportForces.Sum(metric => metric.VerticalPullForce);
                    var maxCompressiveForce = supportForces.Max(metric => metric.CompressiveForce);
                    var maxAngularForce = supportForces.Max(metric => metric.AngularForce);
                    var isOverloaded = verticalPullForce > combinedCapacity;
                    var exceedsCrushForce = maxCompressiveForce > tuning.CrushForceThreshold;
                    var exceedsAngularForce = maxAngularForce > tuning.MaxAngularForce;
                    var exceedsSpacing = furthest.DistanceMm > tuning.SupportSpacingThresholdMm;

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

                    supportPoints.Add(new SupportPoint(
                        new Vec3(furthest.XMm, island.SliceHeightMm, furthest.ZMm),
                        GetTipRadiusMm(newSize, tuning),
                        new Vec3(0f, 0f, 0f),
                        newSize));
                }
            }
        }
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
        var accumulatedForces = new Vec3[supportPoints.Count];

        for (var layerIndex = 0; layerIndex < layerIslands.Length; layerIndex++)
        {
            var prevPixels = prevLayerPixels[layerIndex];
            foreach (var island in layerIslands[layerIndex])
            {
                var unsupportedPixels = GetUnsupportedPixels(island, prevPixels);
                if (unsupportedPixels.Count == 0)
                    continue;

                var supportIndices = FindSupportingSupportsForPixels(
                    unsupportedPixels,
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
                    tuning);

                foreach (var metric in supportForces)
                    accumulatedForces[metric.SupportIndex] = accumulatedForces[metric.SupportIndex] + metric.PullVector;
            }
        }

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
        V3Tuning tuning)
    {
        var pixelAreaMm2 = (bedWidthMm / pixelWidth) * (bedDepthMm / pixelHeight);
        var pixelForce = pixelAreaMm2 * tuning.PeelForceMultiplier;
        var accumulators = supportIndices.ToDictionary(index => index, _ => new SupportForceAccumulator());

        foreach (var (x, y) in unsupportedPixels)
        {
            var xMm = ColumnToX(x, bedWidthMm, pixelWidth);
            var zMm = RowToZ(y, bedDepthMm, pixelHeight);
            var nearestSupportIndex = supportIndices[0];
            var nearestDistanceSq = float.MaxValue;

            foreach (var supportIndex in supportIndices)
            {
                var support = supportPoints[supportIndex];
                var dx = xMm - support.Position.X;
                var dz = zMm - support.Position.Z;
                var distanceSq = (dx * dx) + (dz * dz);
                if (distanceSq < nearestDistanceSq)
                {
                    nearestDistanceSq = distanceSq;
                    nearestSupportIndex = supportIndex;
                }
            }

            var accumulator = accumulators[nearestSupportIndex];
            accumulator.PixelCount++;
            accumulator.SumX += xMm;
            accumulator.SumZ += zMm;
            accumulator.AngularForce += pixelForce * MathF.Sqrt(nearestDistanceSq);
        }

        var metrics = new List<SupportForceMetric>(supportIndices.Count);
        foreach (var supportIndex in supportIndices)
        {
            var accumulator = accumulators[supportIndex];
            if (accumulator.PixelCount == 0)
            {
                metrics.Add(new SupportForceMetric(supportIndex, new Vec3(0f, 0f, 0f), 0f, 0f, 0f));
                continue;
            }

            var support = supportPoints[supportIndex];
            var verticalPullForce = accumulator.PixelCount * pixelForce;
            var centroidX = accumulator.SumX / accumulator.PixelCount;
            var centroidZ = accumulator.SumZ / accumulator.PixelCount;
            var lateralX = (centroidX - support.Position.X) * 0.35f * MathF.Sqrt(MathF.Max(verticalPullForce, 0.01f));
            var lateralZ = (centroidZ - support.Position.Z) * 0.35f * MathF.Sqrt(MathF.Max(verticalPullForce, 0.01f));
            var crushForce = accumulator.AngularForce / MathF.Max(support.RadiusMm, 0.1f);
            var signedVerticalForce = verticalPullForce - crushForce;
            var compressiveForce = MathF.Max(0f, -signedVerticalForce);

            metrics.Add(new SupportForceMetric(
                supportIndex,
                new Vec3(lateralX, signedVerticalForce, lateralZ),
                verticalPullForce,
                compressiveForce,
                accumulator.AngularForce));
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
        float layerHeightMm)
    {
        var indices = new List<int>();
        var maxY = sliceHeightMm + (layerHeightMm * 0.5f);
        var pixelSet = unsupportedPixels.ToHashSet();

        for (var i = 0; i < supportPoints.Count; i++)
        {
            var support = supportPoints[i];
            if (support.Position.Y > maxY)
                continue;

            if (IsSupportInsidePixelSet(support.Position, pixelSet, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight))
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
        int pixelHeight)
    {
        var col = (int)MathF.Floor(((supportPosition.X + (bedWidthMm * 0.5f)) / bedWidthMm) * pixelWidth);
        var row = (int)MathF.Floor((((bedDepthMm * 0.5f) - supportPosition.Z) / bedDepthMm) * pixelHeight);

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
        var bestDist = -1f;
        var bestX = 0f;
        var bestZ = 0f;

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

            if (nearest > bestDist)
            {
                bestDist = nearest;
                bestX = xMm;
                bestZ = zMm;
            }
        }

        return new FurthestPointCandidate(bestX, bestZ, bestDist);
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
        foreach (var support in supportPoints)
            AppendSphere(triangles, support.Position, support.RadiusMm, latSegments: 8, lonSegments: 12);
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

    private V3Tuning ResolveTuning()
    {
        var config = appConfigService?.GetAsync().GetAwaiter().GetResult();
        return new V3Tuning(
            BedMarginMm: config?.AutoSupportBedMarginMm ?? AppConfigService.DefaultAutoSupportBedMarginMm,
            MinVoxelSizeMm: config?.AutoSupportMinVoxelSizeMm ?? AppConfigService.DefaultAutoSupportMinVoxelSizeMm,
            MaxVoxelSizeMm: config?.AutoSupportMaxVoxelSizeMm ?? AppConfigService.DefaultAutoSupportMaxVoxelSizeMm,
            MinLayerHeightMm: config?.AutoSupportMinLayerHeightMm ?? AppConfigService.DefaultAutoSupportMinLayerHeightMm,
            MaxLayerHeightMm: config?.AutoSupportMaxLayerHeightMm ?? AppConfigService.DefaultAutoSupportMaxLayerHeightMm,
            MinIslandAreaMm2: config?.AutoSupportMinIslandAreaMm2 ?? AppConfigService.DefaultAutoSupportMinIslandAreaMm2,
            SupportSpacingThresholdMm: config?.AutoSupportMergeDistanceMm ?? 3f,
            MaxSupportsPerIsland: config?.AutoSupportMaxSupportsPerIsland ?? AppConfigService.DefaultAutoSupportMaxSupportsPerIsland,
            ResinStrength: config?.AutoSupportResinStrength ?? AppConfigService.DefaultAutoSupportResinStrength,
            CrushForceThreshold: config?.AutoSupportCrushForceThreshold ?? AppConfigService.DefaultAutoSupportCrushForceThreshold,
            MaxAngularForce: config?.AutoSupportMaxAngularForce ?? AppConfigService.DefaultAutoSupportMaxAngularForce,
            PeelForceMultiplier: config?.AutoSupportPeelForceMultiplier ?? AppConfigService.DefaultAutoSupportPeelForceMultiplier,
            LightTipRadiusMm: config?.AutoSupportLightTipRadiusMm ?? AppConfigService.DefaultAutoSupportLightTipRadiusMm,
            MediumTipRadiusMm: config?.AutoSupportMediumTipRadiusMm ?? AppConfigService.DefaultAutoSupportMediumTipRadiusMm,
            HeavyTipRadiusMm: config?.AutoSupportHeavyTipRadiusMm ?? AppConfigService.DefaultAutoSupportHeavyTipRadiusMm);
    }

    private sealed record V3Tuning(
        float BedMarginMm,
        float MinVoxelSizeMm,
        float MaxVoxelSizeMm,
        float MinLayerHeightMm,
        float MaxLayerHeightMm,
        float MinIslandAreaMm2,
        float SupportSpacingThresholdMm,
        int MaxSupportsPerIsland,
        float ResinStrength,
        float CrushForceThreshold,
        float MaxAngularForce,
        float PeelForceMultiplier,
        float LightTipRadiusMm,
        float MediumTipRadiusMm,
        float HeavyTipRadiusMm);

    private static float GetTipRadiusMm(SupportSize size, V3Tuning tuning) => size switch
    {
        SupportSize.Heavy => tuning.HeavyTipRadiusMm,
        SupportSize.Medium => tuning.MediumTipRadiusMm,
        _ => tuning.LightTipRadiusMm,
    };

    private sealed record TipIsland(
        List<(int X, int Y)> PixelCoords,
        float CentroidX,
        float CentroidZ,
        float SliceHeightMm,
        float TopLayerAreaMm2);

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
