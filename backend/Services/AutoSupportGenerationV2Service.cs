using System.Diagnostics;
using Microsoft.Extensions.Logging;

namespace findamodel.Services;

/// <summary>
/// Method 2: Force-based voxel autosupport generation.
/// Voxelizes the model at configurable resolution, places supports at bottom-most islands,
/// then iteratively adds supports when accumulated pull or rotational force exceeds
/// capacity. Supports a two-stage coarse/fine optimization pipeline.
/// </summary>
public sealed class AutoSupportGenerationV2Service
{
    private readonly ILogger logger;
    private readonly OrthographicProjectionSliceBitmapGenerator slicer = new();
    private readonly AppConfigService? appConfigService;

    public AutoSupportGenerationV2Service(ILoggerFactory loggerFactory)
        : this(null, loggerFactory)
    {
    }

    public AutoSupportGenerationV2Service(AppConfigService? appConfigService, ILoggerFactory loggerFactory)
    {
        this.appConfigService = appConfigService;
        logger = loggerFactory.CreateLogger<AutoSupportGenerationV2Service>();
    }

    // -----------------------------------------------------------------
    // Public entry point
    // -----------------------------------------------------------------

    public SupportPreviewResult GenerateSupportPreview(LoadedGeometry geometry)
    {
        ArgumentNullException.ThrowIfNull(geometry);

        if (geometry.Triangles.Count == 0)
            return new SupportPreviewResult([], CloneGeometry(geometry, []), []);

        var tuning = ResolveTuning();
        var totalSw = Stopwatch.StartNew();

        SupportPreviewResult result;
        int regionCount = 0;

        if (tuning.OptimizationEnabled && tuning.FineVoxelSizeMm < tuning.CoarseVoxelSizeMm)
        {
            result = GenerateOptimized(geometry, tuning, out regionCount);
        }
        else
        {
            result = GenerateUniform(geometry, tuning, tuning.VoxelSizeMm);
        }

        totalSw.Stop();

        logger.LogInformation(
            "V2: Generated {SupportCount} auto-support markers for model footprint {X:F1}x{Z:F1} mm in {ElapsedMs} ms (optimization={OptEnabled}, regions={RegionCount})",
            result.SupportPoints.Count,
            geometry.DimensionXMm,
            geometry.DimensionZMm,
            totalSw.ElapsedMilliseconds,
            tuning.OptimizationEnabled,
            regionCount);

        return result;
    }

    // -----------------------------------------------------------------
    // Optimized two-stage pipeline
    // -----------------------------------------------------------------

    private SupportPreviewResult GenerateOptimized(LoadedGeometry geometry, Tuning tuning, out int regionCount)
    {
        // Stage 1: Coarse pass
        var coarseSw = Stopwatch.StartNew();
        var coarseResult = RunUniformPass(geometry, tuning, tuning.CoarseVoxelSizeMm);
        coarseSw.Stop();

        // Stage 2: Detect candidate refinement regions
        var candidateRegions = DetectRefinementRegions(
            coarseResult,
            tuning.RefinementMarginMm,
            tuning.RefinementMaxRegions,
            tuning.RiskForceMarginRatio,
            tuning.MinRegionVolumeMm3);

        regionCount = candidateRegions.Count;

        if (candidateRegions.Count == 0)
        {
            // No refinement needed - coarse result is sufficient
            var supportTriangles = BuildSupportSphereMesh(coarseResult.SupportPoints);
            return new SupportPreviewResult(coarseResult.SupportPoints, CloneGeometry(geometry, supportTriangles), coarseResult.DetectedIslands);
        }

        // Stage 3: Regional refinement
        var refinedSupports = new List<SupportPoint>(coarseResult.SupportPoints);
        var spatialIndex = new SupportSpatialIndex(tuning.MaxSupportDistanceMm);
        spatialIndex.Build(refinedSupports);
        var forceCache = new ForceCache();

        foreach (var region in candidateRegions)
        {
            var regionSupports = RunRegionalPass(
                geometry, region, tuning, refinedSupports, spatialIndex);

            // Merge: add new supports from regional pass
            foreach (var sp in regionSupports)
            {
                // Check if this support is too close to an existing one (deterministic tie-breaker: prefer existing)
                var nearby = spatialIndex.FindNearby(sp.Position.X, sp.Position.Z, tuning.SupportMergeDistanceMm, refinedSupports);
                if (nearby.Count > 0)
                    continue;

                var idx = refinedSupports.Count;
                refinedSupports.Add(sp);
                spatialIndex.Insert(idx, sp.Position.X, sp.Position.Z);

                // Invalidate force cache for affected islands
                forceCache.InvalidateAll();
            }
        }

        var finalTriangles = BuildSupportSphereMesh(refinedSupports);
        return new SupportPreviewResult(refinedSupports, CloneGeometry(geometry, finalTriangles), coarseResult.DetectedIslands);
    }

    // -----------------------------------------------------------------
    // Uniform pass (used both standalone and as coarse stage)
    // -----------------------------------------------------------------

    private SupportPreviewResult GenerateUniform(LoadedGeometry geometry, Tuning tuning, float voxelSizeMm)
    {
        var passResult = RunUniformPass(geometry, tuning, voxelSizeMm);
        var supportTriangles = BuildSupportSphereMesh(passResult.SupportPoints);
        return new SupportPreviewResult(passResult.SupportPoints, CloneGeometry(geometry, supportTriangles), passResult.DetectedIslands);
    }

    private UniformPassResult RunUniformPass(LoadedGeometry geometry, Tuning tuning, float voxelSizeMm)
    {
        var bedWidthMm = MathF.Max(geometry.DimensionXMm + (tuning.BedMarginMm * 2f), 10f);
        var bedDepthMm = MathF.Max(geometry.DimensionZMm + (tuning.BedMarginMm * 2f), 10f);
        var gridW = Math.Max(8, (int)Math.Ceiling(bedWidthMm / voxelSizeMm));
        var gridD = Math.Max(8, (int)Math.Ceiling(bedDepthMm / voxelSizeMm));
        var layerCount = Math.Max(1, (int)Math.Ceiling(geometry.DimensionYMm / voxelSizeMm));
        var pixelAreaMm2 = voxelSizeMm * voxelSizeMm;
        var voxelVolumeMm3 = pixelAreaMm2 * voxelSizeMm;

        // Render every layer bitmap upfront
        var layerBitmaps = new SliceBitmap[layerCount];
        for (var i = 0; i < layerCount; i++)
        {
            var h = (i * voxelSizeMm) + (voxelSizeMm * 0.5f);
            layerBitmaps[i] = slicer.RenderLayerBitmap(
                geometry.Triangles, h,
                bedWidthMm, bedDepthMm,
                gridW, gridD,
                voxelSizeMm);
        }

        var prevIslandIds = new int[gridW * gridD];
        var nextIslandId = 1;
        var islandStates = new Dictionary<int, VoxelIslandState>();
        var supportPoints = new List<SupportPoint>();
        var modelHeightMm = geometry.DimensionYMm;
        var riskInfos = new List<IslandRiskInfo>();
        var detectedIslands = new List<IslandPreview>();
        var spatialIndex = new SupportSpatialIndex(tuning.MaxSupportDistanceMm);

        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            var sliceHeightMm = layerIndex * voxelSizeMm;
            var bitmap = layerBitmaps[layerIndex];
            var islands2D = FindLayerIslands(bitmap, gridW, gridD, bedWidthMm, bedDepthMm, voxelSizeMm);
            var curIslandIds = new int[gridW * gridD];

            foreach (var island in islands2D)
            {
                var connected = new HashSet<int>();
                foreach (var cell in island.Cells)
                {
                    var prev = prevIslandIds[cell.Gz * gridW + cell.Gx];
                    if (prev > 0)
                        connected.Add(prev);
                }

                int assignedId;
                if (connected.Count == 0)
                {
                    assignedId = nextIslandId++;
                    islandStates[assignedId] = new VoxelIslandState();
                }
                else
                {
                    assignedId = connected
                        .OrderByDescending(id => islandStates[id].CumulativeVoxelCount)
                        .First();

                    foreach (var otherId in connected)
                    {
                        if (otherId == assignedId) continue;
                        var other = islandStates[otherId];
                        islandStates[assignedId].CumulativeVoxelCount += other.CumulativeVoxelCount;
                        islandStates[assignedId].CumulativePeelForceN += other.CumulativePeelForceN;

                        for (var i = 0; i < prevIslandIds.Length; i++)
                        {
                            if (prevIslandIds[i] == otherId)
                                prevIslandIds[i] = assignedId;
                        }

                        for (var i = 0; i < curIslandIds.Length; i++)
                        {
                            if (curIslandIds[i] == otherId)
                                curIslandIds[i] = assignedId;
                        }

                        islandStates.Remove(otherId);
                    }
                }

                foreach (var cell in island.Cells)
                    curIslandIds[cell.Gz * gridW + cell.Gx] = assignedId;

                // Collect island preview data for all islands
                {
                    var islandAreaForPreview = island.Cells.Count * pixelAreaMm2;
                    var maxRadiusMm = 0f;
                    foreach (var cell in island.Cells)
                    {
                        var dx = cell.XMm - island.CentroidX;
                        var dz = cell.ZMm - island.CentroidZ;
                        maxRadiusMm = MathF.Max(maxRadiusMm, MathF.Sqrt(dx * dx + dz * dz) + voxelSizeMm * 0.5f);
                    }
                    detectedIslands.Add(new IslandPreview(
                        island.CentroidX,
                        island.CentroidZ,
                        sliceHeightMm,
                        islandAreaForPreview,
                        maxRadiusMm));
                }

                var state = islandStates[assignedId];
                state.CumulativeVoxelCount += island.Cells.Count;
                var layerContactArea = island.Cells.Count * pixelAreaMm2;
                state.CumulativePeelForceN += layerContactArea * tuning.PeelForceMultiplier;

                var weightForceN = state.CumulativeVoxelCount * voxelVolumeMm3
                    * (tuning.ResinDensityGPerMl / 1000f)
                    * 0.00981f;
                var totalForceN = weightForceN + state.CumulativePeelForceN;

                // Use spatial index for support lookup
                var islandSupports = FindSupportsInAreaWithIndex(island, supportPoints, tuning.MaxSupportDistanceMm, spatialIndex);

                // Ensure every unsupported island gets at least one support
                if (islandSupports.Count == 0)
                {
                    var islandArea = island.Cells.Count * pixelAreaMm2;
                    var size = islandArea < tuning.MinIslandAreaMm2
                        ? SupportSize.Light
                        : IsNearBase(sliceHeightMm, modelHeightMm)
                            ? SupportSize.Heavy
                            : SupportSize.Medium;
                    var newIdx = supportPoints.Count;
                    supportPoints.Add(new SupportPoint(
                        SnapToVoxelBottom(island.CentroidX, sliceHeightMm, island.CentroidZ),
                        GetTipRadiusMm(size, tuning),
                        new Vec3(0, totalForceN, 0),
                        size));
                    spatialIndex.Insert(newIdx, island.CentroidX, island.CentroidZ);
                    islandSupports = FindSupportsInAreaWithIndex(island, supportPoints, tuning.MaxSupportDistanceMm, spatialIndex);
                }

                if (islandSupports.Count == 0)
                    continue;

                var reinforcementCount = 0;
                float worstExcessForIsland = 0f;

                for (var iter = 0; iter < tuning.MaxSupportsPerIsland; iter++)
                {
                    var forces = DistributeForces(island, islandSupports, supportPoints, totalForceN);

                    var worstIdx = -1;
                    float worstExcess = 0f;

                    foreach (var fe in forces)
                    {
                        var cap = ComputeMaxPullForce(supportPoints[fe.SupportIndex].Size, tuning);
                        var maxTorque = cap * GetTipRadiusMm(supportPoints[fe.SupportIndex].Size, tuning);

                        var forceExcess = fe.PullForceN - cap;
                        var torqueExcess = fe.TorqueNmm - maxTorque;
                        var excess = MathF.Max(forceExcess, 0f) + MathF.Max(torqueExcess, 0f);

                        supportPoints[fe.SupportIndex] = supportPoints[fe.SupportIndex] with
                        {
                            PullForce = new Vec3(fe.LateralX, fe.PullForceN, fe.LateralZ),
                        };

                        if (excess > worstExcess)
                        {
                            worstExcess = excess;
                            worstIdx = fe.SupportIndex;
                        }
                    }

                    worstExcessForIsland = MathF.Max(worstExcessForIsland, worstExcess);

                    if (worstIdx < 0)
                        break;

                    var best = FindBestReinforcementPosition(
                        island, islandSupports, supportPoints, worstIdx, tuning);
                    if (best is null)
                        break;

                    var newSize = IsNearBase(sliceHeightMm, modelHeightMm)
                        ? SupportSize.Heavy
                        : SupportSize.Light;
                    var reinforceIdx = supportPoints.Count;
                    supportPoints.Add(new SupportPoint(
                        SnapToVoxelBottom(best.Value.X, sliceHeightMm, best.Value.Z),
                        GetTipRadiusMm(newSize, tuning),
                        new Vec3(0, 0, 0),
                        newSize));
                    spatialIndex.Insert(reinforceIdx, best.Value.X, best.Value.Z);

                    islandSupports = FindSupportsInAreaWithIndex(island, supportPoints, tuning.MaxSupportDistanceMm, spatialIndex);
                    reinforcementCount++;
                }

                // Track risk info for refinement candidate detection
                var cellMinX = island.Cells.Min(c => c.XMm);
                var cellMaxX = island.Cells.Max(c => c.XMm);
                var cellMinZ = island.Cells.Min(c => c.ZMm);
                var cellMaxZ = island.Cells.Max(c => c.ZMm);
                var unsupportedArea = islandSupports.Count == 0 ? island.Cells.Count * pixelAreaMm2 : 0f;

                riskInfos.Add(new IslandRiskInfo(
                    assignedId,
                    worstExcessForIsland,
                    unsupportedArea,
                    cellMinX, cellMaxX,
                    sliceHeightMm, sliceHeightMm + voxelSizeMm,
                    cellMinZ, cellMaxZ,
                    reinforcementCount));
            }

            prevIslandIds = curIslandIds;
        }

        return new UniformPassResult(supportPoints, islandStates, riskInfos, detectedIslands);
    }

    // -----------------------------------------------------------------
    // Candidate detection for refinement regions
    // -----------------------------------------------------------------

    private static List<RefinementRegion> DetectRefinementRegions(
        UniformPassResult coarseResult,
        float marginMm,
        int maxRegions,
        float riskForceMarginRatio,
        float minRegionVolumeMm3)
    {
        var candidates = new List<RefinementRegion>();

        foreach (var risk in coarseResult.RiskInfos)
        {
            var needsRefinement = false;
            float priority = 0f;

            // High force excess (near capacity)
            if (risk.ForceExcess > 0f)
            {
                needsRefinement = true;
                priority += risk.ForceExcess;
            }

            // High reinforcement churn indicates instability at coarse resolution
            if (risk.ReinforcementChurn >= 3)
            {
                needsRefinement = true;
                priority += risk.ReinforcementChurn * 0.5f;
            }

            // Unsupported area above threshold
            if (risk.UnsupportedAreaMm2 > 0f)
            {
                needsRefinement = true;
                priority += risk.UnsupportedAreaMm2 * 0.1f;
            }

            if (!needsRefinement)
                continue;

            // Expand region by margin
            var region = new RefinementRegion(
                risk.MinX - marginMm, risk.MaxX + marginMm,
                risk.MinY - marginMm, risk.MaxY + marginMm,
                risk.MinZ - marginMm, risk.MaxZ + marginMm,
                priority);

            // Check minimum volume
            var volume = (region.MaxX - region.MinX) * (region.MaxY - region.MinY) * (region.MaxZ - region.MinZ);
            if (volume < minRegionVolumeMm3)
                continue;

            candidates.Add(region);
        }

        // Merge overlapping regions
        candidates = MergeOverlappingRegions(candidates);

        // Sort by priority descending and cap at max
        return candidates
            .OrderByDescending(r => r.Priority)
            .Take(maxRegions)
            .ToList();
    }

    private static List<RefinementRegion> MergeOverlappingRegions(List<RefinementRegion> regions)
    {
        if (regions.Count <= 1)
            return regions;

        var merged = new List<RefinementRegion>();
        var used = new bool[regions.Count];

        for (var i = 0; i < regions.Count; i++)
        {
            if (used[i])
                continue;

            var current = regions[i];
            var didMerge = true;

            while (didMerge)
            {
                didMerge = false;
                for (var j = i + 1; j < regions.Count; j++)
                {
                    if (used[j])
                        continue;

                    var other = regions[j];
                    if (RegionsOverlap(current, other))
                    {
                        current = new RefinementRegion(
                            MathF.Min(current.MinX, other.MinX),
                            MathF.Max(current.MaxX, other.MaxX),
                            MathF.Min(current.MinY, other.MinY),
                            MathF.Max(current.MaxY, other.MaxY),
                            MathF.Min(current.MinZ, other.MinZ),
                            MathF.Max(current.MaxZ, other.MaxZ),
                            MathF.Max(current.Priority, other.Priority));
                        used[j] = true;
                        didMerge = true;
                    }
                }
            }

            merged.Add(current);
        }

        return merged;
    }

    private static bool RegionsOverlap(RefinementRegion a, RefinementRegion b)
        => a.MinX <= b.MaxX && a.MaxX >= b.MinX
        && a.MinY <= b.MaxY && a.MaxY >= b.MinY
        && a.MinZ <= b.MaxZ && a.MaxZ >= b.MinZ;

    // -----------------------------------------------------------------
    // Regional fine pass
    // -----------------------------------------------------------------

    private List<SupportPoint> RunRegionalPass(
        LoadedGeometry geometry,
        RefinementRegion region,
        Tuning tuning,
        List<SupportPoint> existingSupports,
        SupportSpatialIndex spatialIndex)
    {
        var fineVoxel = tuning.FineVoxelSizeMm;

        // Compute regional grid bounds
        var regionWidthMm = region.MaxX - region.MinX;
        var regionDepthMm = region.MaxZ - region.MinZ;
        var regionHeightMm = region.MaxY - region.MinY;

        if (regionWidthMm <= 0 || regionDepthMm <= 0 || regionHeightMm <= 0)
            return [];

        var gridW = Math.Max(2, (int)Math.Ceiling(regionWidthMm / fineVoxel));
        var gridD = Math.Max(2, (int)Math.Ceiling(regionDepthMm / fineVoxel));
        var layerCount = Math.Max(1, (int)Math.Ceiling(regionHeightMm / fineVoxel));
        var pixelAreaMm2 = fineVoxel * fineVoxel;
        var voxelVolumeMm3 = pixelAreaMm2 * fineVoxel;

        // Render layer bitmaps for region only
        var bedWidthMm = regionWidthMm;
        var bedDepthMm = regionDepthMm;
        var layerBitmaps = new SliceBitmap[layerCount];
        for (var i = 0; i < layerCount; i++)
        {
            var h = region.MinY + (i * fineVoxel) + (fineVoxel * 0.5f);
            layerBitmaps[i] = slicer.RenderLayerBitmap(
                geometry.Triangles, h,
                bedWidthMm, bedDepthMm,
                gridW, gridD,
                fineVoxel);
        }

        var newSupports = new List<SupportPoint>();
        var prevIslandIds = new int[gridW * gridD];
        var nextIslandId = 1;
        var islandStates = new Dictionary<int, VoxelIslandState>();
        var modelHeightMm = geometry.DimensionYMm;

        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            var sliceHeightMm = region.MinY + (layerIndex * fineVoxel);
            var bitmap = layerBitmaps[layerIndex];
            var islands2D = FindLayerIslands(bitmap, gridW, gridD, bedWidthMm, bedDepthMm, fineVoxel);
            var curIslandIds = new int[gridW * gridD];

            foreach (var island in islands2D)
            {
                // Offset island cell coordinates to world space
                var worldCells = island.Cells.Select(c => new VoxelCell(
                    c.Gx, c.Gz,
                    region.MinX + ((c.Gx + 0.5f) * fineVoxel),
                    region.MinZ + ((c.Gz + 0.5f) * fineVoxel)
                )).ToList();
                var worldCentroidX = worldCells.Average(c => c.XMm);
                var worldCentroidZ = worldCells.Average(c => c.ZMm);
                var worldIsland = new LayerIsland(worldCells, worldCentroidX, worldCentroidZ);

                var connected = new HashSet<int>();
                foreach (var cell in island.Cells)
                {
                    var prev = prevIslandIds[cell.Gz * gridW + cell.Gx];
                    if (prev > 0)
                        connected.Add(prev);
                }

                int assignedId;
                if (connected.Count == 0)
                {
                    assignedId = nextIslandId++;
                    islandStates[assignedId] = new VoxelIslandState();
                }
                else
                {
                    assignedId = connected
                        .OrderByDescending(id => islandStates[id].CumulativeVoxelCount)
                        .First();
                    foreach (var otherId in connected)
                    {
                        if (otherId == assignedId) continue;
                        var other = islandStates[otherId];
                        islandStates[assignedId].CumulativeVoxelCount += other.CumulativeVoxelCount;
                        islandStates[assignedId].CumulativePeelForceN += other.CumulativePeelForceN;
                        for (var i = 0; i < prevIslandIds.Length; i++)
                            if (prevIslandIds[i] == otherId) prevIslandIds[i] = assignedId;
                        for (var i = 0; i < curIslandIds.Length; i++)
                            if (curIslandIds[i] == otherId) curIslandIds[i] = assignedId;
                        islandStates.Remove(otherId);
                    }
                }

                foreach (var cell in island.Cells)
                    curIslandIds[cell.Gz * gridW + cell.Gx] = assignedId;

                var state = islandStates[assignedId];
                state.CumulativeVoxelCount += island.Cells.Count;
                state.CumulativePeelForceN += island.Cells.Count * pixelAreaMm2 * tuning.PeelForceMultiplier;

                var weightForceN = state.CumulativeVoxelCount * voxelVolumeMm3
                    * (tuning.ResinDensityGPerMl / 1000f) * 0.00981f;
                var totalForceN = weightForceN + state.CumulativePeelForceN;

                // Check existing supports (from coarse pass) using spatial index
                var existingNearby = spatialIndex.FindNearby(worldCentroidX, worldCentroidZ, tuning.MaxSupportDistanceMm, existingSupports);
                var allSupports = new List<SupportPoint>(existingSupports);
                foreach (var ns in newSupports)
                    allSupports.Add(ns);

                var islandSupports = FindSupportsInArea(worldIsland, allSupports, tuning.MaxSupportDistanceMm);

                if (islandSupports.Count == 0
                    && worldCells.Count * pixelAreaMm2 >= tuning.MinIslandAreaMm2)
                {
                    var size = IsNearBase(sliceHeightMm, modelHeightMm)
                        ? SupportSize.Heavy
                        : SupportSize.Medium;
                    newSupports.Add(new SupportPoint(
                        SnapToVoxelBottom(worldCentroidX, sliceHeightMm, worldCentroidZ),
                        GetTipRadiusMm(size, tuning),
                        new Vec3(0, totalForceN, 0),
                        size));
                }
            }

            prevIslandIds = curIslandIds;
        }

        return newSupports;
    }

    // -----------------------------------------------------------------
    // Spatial-index-based support search
    // -----------------------------------------------------------------

    private static List<int> FindSupportsInAreaWithIndex(
        LayerIsland island,
        List<SupportPoint> supportPoints,
        float maxDistanceMm,
        SupportSpatialIndex spatialIndex)
    {
        // Use spatial index for centroid-based initial search
        var nearby = spatialIndex.FindNearby(island.CentroidX, island.CentroidZ, maxDistanceMm * 2f, supportPoints);

        if (nearby.Count == 0)
            return [];

        var result = new List<int>();
        var maxDsq = maxDistanceMm * maxDistanceMm;

        foreach (var idx in nearby)
        {
            var sp = supportPoints[idx];
            // Check if any cell in the island is within range
            foreach (var cell in island.Cells)
            {
                var dx = cell.XMm - sp.Position.X;
                var dz = cell.ZMm - sp.Position.Z;
                if (dx * dx + dz * dz <= maxDsq)
                {
                    result.Add(idx);
                    break;
                }
            }

            if (!result.Contains(idx))
            {
                // Also include if the support is close to the centroid
                var cdx = island.CentroidX - sp.Position.X;
                var cdz = island.CentroidZ - sp.Position.Z;
                if (cdx * cdx + cdz * cdz <= maxDsq)
                    result.Add(idx);
            }
        }

        return result;
    }

    // -----------------------------------------------------------------
    // Voxel-layer island detection (2D connected-component at one layer)
    // -----------------------------------------------------------------

    private static List<LayerIsland> FindLayerIslands(
        SliceBitmap bitmap,
        int gridW,
        int gridD,
        float bedWidthMm,
        float bedDepthMm,
        float voxelSizeMm)
    {
        var visited = new bool[gridW * gridD];
        var islands = new List<LayerIsland>();

        for (var gz = 0; gz < gridD; gz++)
        {
            for (var gx = 0; gx < gridW; gx++)
            {
                var idx = gz * gridW + gx;
                if (visited[idx] || bitmap.Pixels[idx] == 0)
                    continue;

                var queue = new Queue<(int Gx, int Gz)>();
                var cells = new List<VoxelCell>();
                float sumX = 0f, sumZ = 0f;

                visited[idx] = true;
                queue.Enqueue((gx, gz));

                while (queue.Count > 0)
                {
                    var (cx, cz) = queue.Dequeue();
                    var xMm = VoxelCenterX(cx, bedWidthMm, gridW, voxelSizeMm);
                    var zMm = VoxelCenterZ(cz, bedDepthMm, gridD, voxelSizeMm);
                    cells.Add(new VoxelCell(cx, cz, xMm, zMm));
                    sumX += xMm;
                    sumZ += zMm;

                    foreach (var (nx, nz) in Neighbors4(cx, cz))
                    {
                        if (nx < 0 || nz < 0 || nx >= gridW || nz >= gridD) continue;
                        var ni = nz * gridW + nx;
                        if (visited[ni] || bitmap.Pixels[ni] == 0) continue;
                        visited[ni] = true;
                        queue.Enqueue((nx, nz));
                    }
                }

                if (cells.Count == 0) continue;
                islands.Add(new LayerIsland(
                    cells,
                    sumX / cells.Count,
                    sumZ / cells.Count));
            }
        }

        return islands;
    }

    // -----------------------------------------------------------------
    // Force distribution among supports
    // -----------------------------------------------------------------

    private static List<ForceEntry> DistributeForces(
        LayerIsland island,
        List<int> supportIndices,
        List<SupportPoint> supportPoints,
        float totalForceN)
    {
        // Voronoi assignment: each cell goes to nearest support.
        var buckets = supportIndices.ToDictionary(i => i, _ => new ForceBucket());

        foreach (var cell in island.Cells)
        {
            var nearest = supportIndices[0];
            var nearestDsq = float.MaxValue;
            foreach (var si in supportIndices)
            {
                var sp = supportPoints[si];
                var dx = cell.XMm - sp.Position.X;
                var dz = cell.ZMm - sp.Position.Z;
                var dsq = dx * dx + dz * dz;
                if (dsq < nearestDsq) { nearestDsq = dsq; nearest = si; }
            }

            buckets[nearest].Count++;
            buckets[nearest].SumX += cell.XMm;
            buckets[nearest].SumZ += cell.ZMm;
        }

        var totalCells = island.Cells.Count;
        var result = new List<ForceEntry>(supportIndices.Count);

        foreach (var (si, bucket) in buckets)
        {
            if (bucket.Count == 0)
            {
                result.Add(new ForceEntry(si, 0f, 0f, 0f, 0f));
                continue;
            }

            var fraction = (float)bucket.Count / totalCells;
            var pullForce = totalForceN * fraction;

            var centX = bucket.SumX / bucket.Count;
            var centZ = bucket.SumZ / bucket.Count;
            var sp = supportPoints[si];
            var armX = centX - sp.Position.X;
            var armZ = centZ - sp.Position.Z;
            var armLength = MathF.Sqrt(armX * armX + armZ * armZ);

            // Torque = force * lever arm distance (N*mm)
            var torque = pullForce * armLength;

            // Lateral force components (proportional to offset)
            var lateralX = armX * 0.35f * MathF.Sqrt(MathF.Max(pullForce, 0.01f));
            var lateralZ = armZ * 0.35f * MathF.Sqrt(MathF.Max(pullForce, 0.01f));

            result.Add(new ForceEntry(si, pullForce, torque, lateralX, lateralZ));
        }

        return result;
    }

    // -----------------------------------------------------------------
    // Optimal reinforcement position
    // -----------------------------------------------------------------

    /// <summary>
    /// Finds the voxel position within the island that, if a support were placed
    /// there, would maximally reduce the pull force on the most-stressed support.
    /// </summary>
    private static (float X, float Z)? FindBestReinforcementPosition(
        LayerIsland island,
        List<int> supportIndices,
        List<SupportPoint> supportPoints,
        int worstSupportIndex,
        Tuning tuning)
    {
        // Collect cells currently assigned to the worst support.
        var worstCells = new List<VoxelCell>();
        var sp = supportPoints[worstSupportIndex];

        foreach (var cell in island.Cells)
        {
            var nearest = supportIndices[0];
            var nearestDsq = float.MaxValue;
            foreach (var si in supportIndices)
            {
                var p = supportPoints[si];
                var dx = cell.XMm - p.Position.X;
                var dz = cell.ZMm - p.Position.Z;
                var dsq = dx * dx + dz * dz;
                if (dsq < nearestDsq) { nearestDsq = dsq; nearest = si; }
            }

            if (nearest == worstSupportIndex)
                worstCells.Add(cell);
        }

        if (worstCells.Count == 0)
            return null;

        // Candidate: centroid of the worst support's Voronoi region.
        float bestX = 0f, bestZ = 0f;
        float bestScore = float.MinValue;
        var evaluated = false;

        void Evaluate(float cx, float cz)
        {
            // Reject if too close to any existing support.
            foreach (var si in supportIndices)
            {
                var p = supportPoints[si];
                var dx = cx - p.Position.X;
                var dz = cz - p.Position.Z;
                if (dx * dx + dz * dz < tuning.SupportMergeDistanceMm * tuning.SupportMergeDistanceMm)
                    return;
            }

            // Score = number of cells that would be re-assigned from the worst support
            // to the new candidate, weighted by distance improvement.
            float score = 0f;
            foreach (var cell in worstCells)
            {
                var dxOld = cell.XMm - sp.Position.X;
                var dzOld = cell.ZMm - sp.Position.Z;
                var distOldSq = dxOld * dxOld + dzOld * dzOld;

                var dxNew = cell.XMm - cx;
                var dzNew = cell.ZMm - cz;
                var distNewSq = dxNew * dxNew + dzNew * dzNew;

                if (distNewSq < distOldSq)
                    score += MathF.Sqrt(distOldSq) - MathF.Sqrt(distNewSq);
            }

            if (score > bestScore)
            {
                bestScore = score;
                bestX = cx;
                bestZ = cz;
                evaluated = true;
            }
        }

        // Centroid of the worst region.
        var centX = worstCells.Average(c => c.XMm);
        var centZ = worstCells.Average(c => c.ZMm);
        Evaluate(centX, centZ);

        // Farthest cell from the worst support.
        var farthest = worstCells.MaxBy(c =>
        {
            var dx = c.XMm - sp.Position.X;
            var dz = c.ZMm - sp.Position.Z;
            return dx * dx + dz * dz;
        });
        if (farthest != null)
            Evaluate(farthest.XMm, farthest.ZMm);

        // Sample a subset of cells in the worst region.
        var step = Math.Max(1, worstCells.Count / 32);
        for (var i = 0; i < worstCells.Count; i += step)
            Evaluate(worstCells[i].XMm, worstCells[i].ZMm);

        return evaluated ? (bestX, bestZ) : null;
    }

    // -----------------------------------------------------------------
    // Support search helpers
    // -----------------------------------------------------------------

    private static List<int> FindSupportsInArea(
        LayerIsland island,
        List<SupportPoint> supportPoints,
        float maxDistanceMm)
    {
        var result = new List<int>();
        var maxDsq = maxDistanceMm * maxDistanceMm;

        for (var i = 0; i < supportPoints.Count; i++)
        {
            var sp = supportPoints[i];
            // Check if support is within max distance of island centroid
            // (use generous island radius estimate).
            var maxCellDist = 0f;
            foreach (var cell in island.Cells)
            {
                var dx = cell.XMm - sp.Position.X;
                var dz = cell.ZMm - sp.Position.Z;
                var dsq = dx * dx + dz * dz;
                if (dsq <= maxDsq)
                {
                    result.Add(i);
                    goto next;
                }

                maxCellDist = MathF.Max(maxCellDist, MathF.Sqrt(dsq));
            }

            // Also include if the support is close to the centroid.
            {
                var dx = island.CentroidX - sp.Position.X;
                var dz = island.CentroidZ - sp.Position.Z;
                if (dx * dx + dz * dz <= maxDsq)
                    result.Add(i);
            }

        next:;
        }

        return result;
    }

    // -----------------------------------------------------------------
    // Coordinate helpers
    // -----------------------------------------------------------------

    private static float VoxelCenterX(int gx, float bedWidthMm, int gridW, float voxelSizeMm)
        => ((gx + 0.5f) * voxelSizeMm) - (bedWidthMm * 0.5f);

    private static float VoxelCenterZ(int gz, float bedDepthMm, int gridD, float voxelSizeMm)
        => (bedDepthMm * 0.5f) - ((gz + 0.5f) * voxelSizeMm);

    private static Vec3 SnapToVoxelBottom(float xMm, float yBottomMm, float zMm)
        => new(xMm, yBottomMm, zMm);

    private static IEnumerable<(int X, int Z)> Neighbors4(int x, int z)
    {
        yield return (x - 1, z);
        yield return (x + 1, z);
        yield return (x, z - 1);
        yield return (x, z + 1);
    }

    // -----------------------------------------------------------------
    // Shared utilities (same as V1)
    // -----------------------------------------------------------------

    private static LoadedGeometry CloneGeometry(LoadedGeometry geometry, List<Triangle3D> triangles) => new()
    {
        Triangles = triangles,
        DimensionXMm = geometry.DimensionXMm,
        DimensionYMm = geometry.DimensionYMm,
        DimensionZMm = geometry.DimensionZMm,
        SphereCentre = geometry.SphereCentre,
        SphereRadius = geometry.SphereRadius,
    };

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

    // -----------------------------------------------------------------
    // Tuning / physics helpers
    // -----------------------------------------------------------------

    private static float GetTipRadiusMm(SupportSize size, Tuning tuning) => size switch
    {
        SupportSize.Micro => tuning.MicroTipRadiusMm,
        SupportSize.Light => tuning.LightTipRadiusMm,
        SupportSize.Medium => tuning.MediumTipRadiusMm,
        SupportSize.Heavy => tuning.HeavyTipRadiusMm,
        _ => tuning.MediumTipRadiusMm,
    };

    private static float ComputeMaxPullForce(SupportSize size, Tuning tuning)
    {
        var r = GetTipRadiusMm(size, tuning);
        return MathF.PI * r * r * tuning.ResinStrength;
    }

    private static bool IsNearBase(float sliceHeightMm, float modelHeightMm)
        => sliceHeightMm <= MathF.Max(modelHeightMm * 0.15f, 3f);

    private Tuning ResolveTuning()
    {
        var config = appConfigService?.GetAsync().GetAwaiter().GetResult();
        return new Tuning(
            VoxelSizeMm: config?.AutoSupportV2VoxelSizeMm ?? AppConfigService.DefaultAutoSupportV2VoxelSizeMm,
            BedMarginMm: config?.AutoSupportBedMarginMm ?? AppConfigService.DefaultAutoSupportBedMarginMm,
            MaxSupportDistanceMm: config?.AutoSupportMaxSupportDistanceMm ?? AppConfigService.DefaultAutoSupportMaxSupportDistanceMm,
            SupportMergeDistanceMm: config?.AutoSupportMergeDistanceMm ?? AppConfigService.DefaultAutoSupportMergeDistanceMm,
            MinIslandAreaMm2: config?.AutoSupportMinIslandAreaMm2 ?? AppConfigService.DefaultAutoSupportMinIslandAreaMm2,
            ResinStrength: config?.AutoSupportResinStrength ?? AppConfigService.DefaultAutoSupportResinStrength,
            ResinDensityGPerMl: config?.AutoSupportResinDensityGPerMl ?? AppConfigService.DefaultAutoSupportResinDensityGPerMl,
            PeelForceMultiplier: config?.AutoSupportPeelForceMultiplier ?? AppConfigService.DefaultAutoSupportPeelForceMultiplier,
            MicroTipRadiusMm: config?.AutoSupportMicroTipRadiusMm ?? AppConfigService.DefaultAutoSupportMicroTipRadiusMm,
            LightTipRadiusMm: config?.AutoSupportLightTipRadiusMm ?? AppConfigService.DefaultAutoSupportLightTipRadiusMm,
            MediumTipRadiusMm: config?.AutoSupportMediumTipRadiusMm ?? AppConfigService.DefaultAutoSupportMediumTipRadiusMm,
            HeavyTipRadiusMm: config?.AutoSupportHeavyTipRadiusMm ?? AppConfigService.DefaultAutoSupportHeavyTipRadiusMm,
            MaxSupportsPerIsland: config?.AutoSupportMaxSupportsPerIsland ?? AppConfigService.DefaultAutoSupportMaxSupportsPerIsland,
            OptimizationEnabled: config?.AutoSupportV2OptimizationEnabled ?? AppConfigService.DefaultAutoSupportV2OptimizationEnabled,
            CoarseVoxelSizeMm: config?.AutoSupportV2CoarseVoxelSizeMm ?? AppConfigService.DefaultAutoSupportV2CoarseVoxelSizeMm,
            FineVoxelSizeMm: config?.AutoSupportV2FineVoxelSizeMm ?? AppConfigService.DefaultAutoSupportV2FineVoxelSizeMm,
            RefinementMarginMm: config?.AutoSupportV2RefinementMarginMm ?? AppConfigService.DefaultAutoSupportV2RefinementMarginMm,
            RefinementMaxRegions: config?.AutoSupportV2RefinementMaxRegions ?? AppConfigService.DefaultAutoSupportV2RefinementMaxRegions,
            RiskForceMarginRatio: config?.AutoSupportV2RiskForceMarginRatio ?? AppConfigService.DefaultAutoSupportV2RiskForceMarginRatio,
            MinRegionVolumeMm3: config?.AutoSupportV2MinRegionVolumeMm3 ?? AppConfigService.DefaultAutoSupportV2MinRegionVolumeMm3);
    }

    // -----------------------------------------------------------------
    // Internal types
    // -----------------------------------------------------------------

    private sealed record Tuning(
        float VoxelSizeMm,
        float BedMarginMm,
        float MaxSupportDistanceMm,
        float SupportMergeDistanceMm,
        float MinIslandAreaMm2,
        float ResinStrength,
        float ResinDensityGPerMl,
        float PeelForceMultiplier,
        float MicroTipRadiusMm,
        float LightTipRadiusMm,
        float MediumTipRadiusMm,
        float HeavyTipRadiusMm,
        int MaxSupportsPerIsland,
        bool OptimizationEnabled,
        float CoarseVoxelSizeMm,
        float FineVoxelSizeMm,
        float RefinementMarginMm,
        int RefinementMaxRegions,
        float RiskForceMarginRatio,
        float MinRegionVolumeMm3);

    private sealed record VoxelCell(int Gx, int Gz, float XMm, float ZMm);

    private sealed record LayerIsland(
        List<VoxelCell> Cells,
        float CentroidX,
        float CentroidZ);

    private sealed class VoxelIslandState
    {
        public int CumulativeVoxelCount;
        public float CumulativePeelForceN;
    }

    private sealed record ForceEntry(
        int SupportIndex,
        float PullForceN,
        float TorqueNmm,
        float LateralX,
        float LateralZ);

    private sealed class ForceBucket
    {
        public int Count;
        public float SumX;
        public float SumZ;
    }

    // -----------------------------------------------------------------
    // Optimization types
    // -----------------------------------------------------------------

    private sealed record RefinementRegion(
        float MinX, float MaxX,
        float MinY, float MaxY,
        float MinZ, float MaxZ,
        float Priority);

    private sealed record UniformPassResult(
        List<SupportPoint> SupportPoints,
        Dictionary<int, VoxelIslandState> IslandStates,
        List<IslandRiskInfo> RiskInfos,
        List<IslandPreview> DetectedIslands);

    private sealed record IslandRiskInfo(
        int IslandId,
        float ForceExcess,
        float UnsupportedAreaMm2,
        float MinX, float MaxX,
        float MinY, float MaxY,
        float MinZ, float MaxZ,
        int ReinforcementChurn);

    // -----------------------------------------------------------------
    // Spatial hash index for nearest-support lookups
    // -----------------------------------------------------------------

    internal sealed class SupportSpatialIndex
    {
        private readonly float cellSize;
        private readonly Dictionary<(int Cx, int Cz), List<int>> cells = new();

        public SupportSpatialIndex(float cellSize)
        {
            this.cellSize = MathF.Max(cellSize, 0.1f);
        }

        public void Clear()
        {
            cells.Clear();
        }

        public void Build(List<SupportPoint> supportPoints)
        {
            cells.Clear();
            for (var i = 0; i < supportPoints.Count; i++)
                Insert(i, supportPoints[i].Position.X, supportPoints[i].Position.Z);
        }

        public void Insert(int index, float x, float z)
        {
            var key = CellKey(x, z);
            if (!cells.TryGetValue(key, out var list))
            {
                list = new List<int>();
                cells[key] = list;
            }
            list.Add(index);
        }

        public List<int> FindNearby(float x, float z, float radius, List<SupportPoint> supportPoints)
        {
            var result = new List<int>();
            var radiusSq = radius * radius;
            var cellRadius = (int)MathF.Ceiling(radius / cellSize);
            var (baseCx, baseCz) = CellKey(x, z);

            for (var dx = -cellRadius; dx <= cellRadius; dx++)
            {
                for (var dz = -cellRadius; dz <= cellRadius; dz++)
                {
                    var key = (baseCx + dx, baseCz + dz);
                    if (!cells.TryGetValue(key, out var list))
                        continue;

                    foreach (var idx in list)
                    {
                        var sp = supportPoints[idx];
                        var ddx = sp.Position.X - x;
                        var ddz = sp.Position.Z - z;
                        if (ddx * ddx + ddz * ddz <= radiusSq)
                            result.Add(idx);
                    }
                }
            }

            return result;
        }

        private (int, int) CellKey(float x, float z)
            => ((int)MathF.Floor(x / cellSize), (int)MathF.Floor(z / cellSize));
    }

    // -----------------------------------------------------------------
    // Force cache for incremental recomputation
    // -----------------------------------------------------------------

    private sealed class ForceCache
    {
        private readonly Dictionary<int, ForceCacheEntry> entries = new();

        public void Invalidate(int islandId)
        {
            entries.Remove(islandId);
        }

        public void InvalidateAll()
        {
            entries.Clear();
        }

        public bool TryGet(int islandId, int supportSetHash, out ForceCacheEntry entry)
        {
            if (entries.TryGetValue(islandId, out entry!) && entry.SupportSetHash == supportSetHash)
                return true;
            entry = default!;
            return false;
        }

        public void Set(int islandId, int supportSetHash, float worstExcess, int worstSupportIndex)
        {
            entries[islandId] = new ForceCacheEntry(supportSetHash, worstExcess, worstSupportIndex);
        }
    }

    private sealed record ForceCacheEntry(int SupportSetHash, float WorstExcess, int WorstSupportIndex);

    // -----------------------------------------------------------------
    // Profiling metrics
    // -----------------------------------------------------------------

    public sealed record V2Metrics(
        long TotalRuntimeMs,
        long LayerRenderingMs,
        long IslandDetectionMs,
        long ForceEvaluationMs,
        int SupportsAdded,
        int SupportsReinforced,
        long EstimatedActiveVoxels,
        int RefinementRegionCount,
        bool OptimizationEnabled);
}
