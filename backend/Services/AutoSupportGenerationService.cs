using Microsoft.Extensions.Logging;

namespace findamodel.Services;

public sealed class AutoSupportGenerationService
{
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

    public SupportPreviewResult GenerateSupportPreview(LoadedGeometry geometry)
    {
        ArgumentNullException.ThrowIfNull(geometry);

        if (geometry.Triangles.Count == 0)
            return new SupportPreviewResult([], CloneGeometry(geometry, []));

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
        var supportPoints = new List<SupportPoint>();
        var modelHeightMm = geometry.DimensionYMm;
        var cumulativeVolumePerPixelMm3 = 0f;

        var layerCount = Math.Max(1, (int)Math.Ceiling(Math.Max(geometry.DimensionYMm, layerHeightMm) / layerHeightMm));
        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
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

            var litPixelCount = bitmap.CountLitPixels();
            if (litPixelCount == 0)
                continue;

            var pixelWidthMm = bedWidthMm / pixelWidth;
            var pixelDepthMm = bedDepthMm / pixelHeight;
            var pixelAreaMm2 = pixelWidthMm * pixelDepthMm;

            // Accumulate volume for this layer (each lit pixel represents a voxel column)
            cumulativeVolumePerPixelMm3 += layerHeightMm;

            // Weight force: cumulative volume * density converted to grams then to Newtons (g / 1000 * 9.81)
            var cumulativeWeightN = litPixelCount * cumulativeVolumePerPixelMm3 * pixelAreaMm2
                * (tuning.ResinDensityGPerMl / 1000f)  // mm3 -> ml = /1000, * g/ml -> grams
                * 0.00981f;                              // grams -> Newtons (g * 9.81 / 1000)

            // Peel force: proportional to cross-sectional area of this layer
            var layerAreaMm2 = litPixelCount * pixelAreaMm2;
            var peelForceN = layerAreaMm2 * tuning.PeelForceMultiplier;

            var layerForces = new LayerForces(cumulativeWeightN, peelForceN);

            foreach (var island in FindIslands(
                         bitmap,
                         bedWidthMm,
                         bedDepthMm,
                         sliceHeightMm,
                         tuning.SupportMergeDistanceMm,
                         tuning.MinIslandAreaMm2))
            {
                EnsureInitialSupport(island, supportPoints, tuning, modelHeightMm);
                ReinforceIslandIfNeeded(island, supportPoints, tuning, modelHeightMm, layerForces, litPixelCount);
            }
        }

        var supportTriangles = BuildSupportSphereMesh(supportPoints);
        logger.LogInformation(
            "Generated {SupportCount} auto-support markers for model footprint {X:F1}x{Z:F1} mm",
            supportPoints.Count,
            geometry.DimensionXMm,
            geometry.DimensionZMm);

        return new SupportPreviewResult(
            supportPoints,
            CloneGeometry(geometry, supportTriangles));
    }

    private static LoadedGeometry CloneGeometry(LoadedGeometry geometry, List<Triangle3D> triangles) => new()
    {
        Triangles = triangles,
        DimensionXMm = geometry.DimensionXMm,
        DimensionYMm = geometry.DimensionYMm,
        DimensionZMm = geometry.DimensionZMm,
        SphereCentre = geometry.SphereCentre,
        SphereRadius = geometry.SphereRadius,
    };

    private static void EnsureInitialSupport(SliceIsland island, List<SupportPoint> supportPoints, AutoSupportTuning tuning, float modelHeightMm)
    {
        if (FindSupportsInsideIsland(island, supportPoints).Count > 0)
            return;

        var size = IsNearBase(island.SliceHeightMm, modelHeightMm) ? SupportSize.Heavy : SupportSize.Medium;
        supportPoints.Add(new SupportPoint(
            new Vec3(island.CentroidX, island.SliceHeightMm, island.CentroidZ),
            GetTipRadiusMm(size, tuning),
            new Vec3(0f, 0f, 0f),
            size));
    }

    private static void ReinforceIslandIfNeeded(SliceIsland island, List<SupportPoint> supportPoints, AutoSupportTuning tuning, float modelHeightMm, LayerForces layerForces, int totalLitPixels)
    {
        for (var iteration = 0; iteration < tuning.MaxSupportsPerIsland; iteration++)
        {
            var islandSupportIndices = FindSupportsInsideIsland(island, supportPoints);
            if (islandSupportIndices.Count == 0)
            {
                EnsureInitialSupport(island, supportPoints, tuning, modelHeightMm);
                islandSupportIndices = FindSupportsInsideIsland(island, supportPoints);
                if (islandSupportIndices.Count == 0)
                    return;
            }

            var strongestPull = EvaluatePullForces(island, islandSupportIndices, supportPoints, layerForces, totalLitPixels);
            var uncoveredPixels = FindPixelsBeyondMaxDistance(
                island,
                islandSupportIndices,
                supportPoints,
                tuning.MaxSupportDistanceMm);
            var needsAdditionalCoverage = uncoveredPixels.Count > 0;

            var currentSize = strongestPull.SupportIndex >= 0
                ? supportPoints[strongestPull.SupportIndex].Size
                : SupportSize.Medium;
            var maxCapacity = ComputeMaxPullForce(currentSize, tuning);
            var forceExceeded = strongestPull.Score > maxCapacity;

            if (!needsAdditionalCoverage && !forceExceeded)
                return;

            if (forceExceeded && strongestPull.SupportIndex >= 0)
            {
                var nearbyCount = CountNearbySupports(
                    supportPoints[strongestPull.SupportIndex].Position,
                    supportPoints,
                    tuning.MaxSupportDistanceMm,
                    strongestPull.SupportIndex);

                if (nearbyCount >= 3 && currentSize != SupportSize.Heavy)
                {
                    var newSize = NextLargerSize(currentSize);
                    var existing = supportPoints[strongestPull.SupportIndex];
                    supportPoints[strongestPull.SupportIndex] = existing with
                    {
                        Size = newSize,
                        RadiusMm = GetTipRadiusMm(newSize, tuning),
                        PullForce = strongestPull.Vector,
                    };
                    continue;
                }
            }

            var candidatePoint = FindBestAdditionalSupportPoint(island, islandSupportIndices, supportPoints, tuning);
            if (candidatePoint is null)
                return;

            var respectsMinDistance = candidatePoint.DistanceMm >= tuning.SupportMergeDistanceMm;
            var newSupportSize = IsNearBase(island.SliceHeightMm, modelHeightMm)
                ? SupportSize.Heavy
                : SupportSize.Light;
            if (respectsMinDistance)
            {
                supportPoints.Add(new SupportPoint(
                    new Vec3(candidatePoint.X, island.SliceHeightMm, candidatePoint.Z),
                    GetTipRadiusMm(newSupportSize, tuning),
                    strongestPull.Vector,
                    newSupportSize));
                continue;
            }

            if (TryRepositionSupport(island, islandSupportIndices, supportPoints, tuning, candidatePoint, strongestPull.Vector))
                continue;

            if (!needsAdditionalCoverage)
                return;

            supportPoints.Add(new SupportPoint(
                new Vec3(candidatePoint.X, island.SliceHeightMm, candidatePoint.Z),
                GetTipRadiusMm(newSupportSize, tuning),
                strongestPull.Vector,
                newSupportSize));
        }
    }

    private static PullForceEstimate EvaluatePullForces(
        SliceIsland island,
        List<int> islandSupportIndices,
        List<SupportPoint> supportPoints,
        LayerForces layerForces,
        int totalLitPixels)
    {
        var pixelAreaMm2 = island.PixelAreaMm2;
        var supportAssignments = islandSupportIndices.ToDictionary(index => index, _ => new SupportAccumulator());

        foreach (var pixel in island.Pixels)
        {
            var nearestIndex = islandSupportIndices[0];
            var nearestDistanceSq = float.MaxValue;
            foreach (var supportIndex in islandSupportIndices)
            {
                var support = supportPoints[supportIndex];
                var dx = pixel.XMm - support.Position.X;
                var dz = pixel.ZMm - support.Position.Z;
                var distanceSq = (dx * dx) + (dz * dz);
                if (distanceSq < nearestDistanceSq)
                {
                    nearestDistanceSq = distanceSq;
                    nearestIndex = supportIndex;
                }
            }

            var accumulator = supportAssignments[nearestIndex];
            accumulator.Count++;
            accumulator.SumX += pixel.XMm;
            accumulator.SumZ += pixel.ZMm;
            accumulator.SumDistance += MathF.Sqrt(nearestDistanceSq);
        }

        var best = new PullForceEstimate(-1, 0f, new Vec3(0f, 0f, 0f));
        foreach (var pair in supportAssignments)
        {
            var supportIndex = pair.Key;
            var accumulator = pair.Value;
            if (accumulator.Count == 0)
                continue;

            var support = supportPoints[supportIndex];
            var centroidX = accumulator.SumX / accumulator.Count;
            var centroidZ = accumulator.SumZ / accumulator.Count;
            var averageDistanceMm = accumulator.SumDistance / accumulator.Count;
            var supportedAreaMm2 = accumulator.Count * pixelAreaMm2;
            var verticalComponent = MathF.Sqrt(MathF.Max(supportedAreaMm2, 0.01f));
            var lateralX = (centroidX - support.Position.X) * 0.35f * verticalComponent;
            var lateralZ = (centroidZ - support.Position.Z) * 0.35f * verticalComponent;

            // Distribute cumulative weight and peel forces proportionally to this support's pixel share
            var pixelFraction = totalLitPixels > 0
                ? (float)accumulator.Count / totalLitPixels
                : 0f;
            var weightContribution = layerForces.CumulativeWeightN * pixelFraction;
            var peelContribution = layerForces.PeelForceN * pixelFraction;
            var physicsForce = weightContribution + peelContribution;

            var vector = new Vec3(lateralX, verticalComponent + physicsForce, lateralZ);
            var score = vector.Length + (averageDistanceMm * 1.5f);

            supportPoints[supportIndex] = support with { PullForce = vector };
            if (score > best.Score)
                best = new PullForceEstimate(supportIndex, score, vector);
        }

        return best;
    }

    private static AdditionalSupportCandidate? FindBestAdditionalSupportPoint(
        SliceIsland island,
        IReadOnlyList<int> islandSupportIndices,
        List<SupportPoint> supportPoints,
        AutoSupportTuning tuning)
    {
        var uncoveredPixels = FindPixelsBeyondMaxDistance(island, islandSupportIndices, supportPoints, tuning.MaxSupportDistanceMm);
        var focusPixels = uncoveredPixels.Count > 0 ? uncoveredPixels : island.Pixels;
        var boundaryStep = Math.Max(1, island.BoundaryPoints.Count / 48);
        var pixelStep = Math.Max(1, focusPixels.Count / 64);
        AdditionalSupportCandidate? bestPoint = null;
        var bestScore = float.MinValue;

        void EvaluateCandidate(float xMm, float zMm)
        {
            var nearestDistanceMm = FindNearestSupportDistance(xMm, zMm, supportPoints);
            if (nearestDistanceMm < 0.05f)
                return;

            var newlyCoveredPixels = 0;
            var totalCoveredPixels = 0;
            var improvementScore = 0f;
            foreach (var pixel in island.Pixels)
            {
                var currentDistanceMm = FindNearestPixelDistance(pixel, islandSupportIndices, supportPoints);
                var dx = pixel.XMm - xMm;
                var dz = pixel.ZMm - zMm;
                var candidateDistanceMm = MathF.Sqrt((dx * dx) + (dz * dz));
                if (candidateDistanceMm <= tuning.MaxSupportDistanceMm)
                {
                    totalCoveredPixels++;
                    if (currentDistanceMm > tuning.MaxSupportDistanceMm)
                        newlyCoveredPixels++;
                }

                improvementScore += MathF.Max(0f, currentDistanceMm - candidateDistanceMm);
            }

            if (newlyCoveredPixels == 0 && improvementScore < 1f)
                return;

            var spacingPenalty = nearestDistanceMm < tuning.SupportMergeDistanceMm
                ? (tuning.SupportMergeDistanceMm - nearestDistanceMm) * 250f
                : 0f;
            var score = (newlyCoveredPixels * 1000f) + (totalCoveredPixels * 5f) + improvementScore - spacingPenalty;
            if (score <= bestScore)
                return;

            bestScore = score;
            bestPoint = new AdditionalSupportCandidate(xMm, zMm, nearestDistanceMm);
        }

        EvaluateCandidate(island.CentroidX, island.CentroidZ);

        for (var i = 0; i < island.BoundaryPoints.Count; i += boundaryStep)
        {
            var point = island.BoundaryPoints[i];
            EvaluateCandidate(point.X, point.Z);
        }

        for (var i = 0; i < focusPixels.Count; i += pixelStep)
        {
            var pixel = focusPixels[i];
            EvaluateCandidate(pixel.XMm, pixel.ZMm);
        }

        return bestPoint;
    }

    private static bool TryRepositionSupport(
        SliceIsland island,
        IReadOnlyList<int> islandSupportIndices,
        List<SupportPoint> supportPoints,
        AutoSupportTuning tuning,
        AdditionalSupportCandidate candidate,
        Vec3 pullForce)
    {
        foreach (var supportIndex in islandSupportIndices)
        {
            if (FindNearestSupportDistance(candidate.X, candidate.Z, supportPoints, supportIndex) < tuning.SupportMergeDistanceMm)
                continue;

            var existing = supportPoints[supportIndex];
            supportPoints[supportIndex] = existing with
            {
                Position = new Vec3(candidate.X, island.SliceHeightMm, candidate.Z),
                PullForce = pullForce,
            };

            var updatedIndices = FindSupportsInsideIsland(island, supportPoints);
            var uncoveredPixels = FindPixelsBeyondMaxDistance(
                island,
                updatedIndices,
                supportPoints,
                tuning.MaxSupportDistanceMm);
            if (uncoveredPixels.Count == 0)
                return true;

            supportPoints[supportIndex] = existing;
        }

        return false;
    }

    private static List<SlicePixel> FindPixelsBeyondMaxDistance(
        SliceIsland island,
        IReadOnlyList<int> islandSupportIndices,
        List<SupportPoint> supportPoints,
        float maxSupportDistanceMm)
    {
        var result = new List<SlicePixel>();
        foreach (var pixel in island.Pixels)
        {
            var nearestDistanceMm = FindNearestPixelDistance(pixel, islandSupportIndices, supportPoints);
            if (nearestDistanceMm > maxSupportDistanceMm)
                result.Add(pixel);
        }

        return result;
    }

    private static float FindNearestPixelDistance(
        SlicePixel pixel,
        IReadOnlyList<int> islandSupportIndices,
        List<SupportPoint> supportPoints)
    {
        if (islandSupportIndices.Count == 0)
            return float.MaxValue;

        var nearestDistanceSq = float.MaxValue;
        foreach (var supportIndex in islandSupportIndices)
        {
            var support = supportPoints[supportIndex];
            var dx = pixel.XMm - support.Position.X;
            var dz = pixel.ZMm - support.Position.Z;
            var distanceSq = (dx * dx) + (dz * dz);
            if (distanceSq < nearestDistanceSq)
                nearestDistanceSq = distanceSq;
        }

        return MathF.Sqrt(nearestDistanceSq);
    }

    private static List<int> FindSupportsInsideIsland(SliceIsland island, List<SupportPoint> supportPoints)
    {
        var result = new List<int>();
        var limitSq = island.IslandRadiusMm * island.IslandRadiusMm;

        for (var i = 0; i < supportPoints.Count; i++)
        {
            var support = supportPoints[i];
            var dx = support.Position.X - island.CentroidX;
            var dz = support.Position.Z - island.CentroidZ;
            var distanceSq = (dx * dx) + (dz * dz);
            if (distanceSq <= limitSq)
                result.Add(i);
        }

        return result;
    }

    private static int FindNearestSupportIndex(float xMm, float zMm, List<SupportPoint> supportPoints, float maxDistanceMm)
    {
        var maxDistanceSq = maxDistanceMm * maxDistanceMm;
        for (var i = 0; i < supportPoints.Count; i++)
        {
            var dx = supportPoints[i].Position.X - xMm;
            var dz = supportPoints[i].Position.Z - zMm;
            var distanceSq = (dx * dx) + (dz * dz);
            if (distanceSq <= maxDistanceSq)
                return i;
        }

        return -1;
    }

    private static float FindNearestSupportDistance(float xMm, float zMm, List<SupportPoint> supportPoints, int excludedSupportIndex = -1)
    {
        var nearestDistanceSq = float.MaxValue;
        for (var i = 0; i < supportPoints.Count; i++)
        {
            if (i == excludedSupportIndex)
                continue;

            var dx = supportPoints[i].Position.X - xMm;
            var dz = supportPoints[i].Position.Z - zMm;
            var distanceSq = (dx * dx) + (dz * dz);
            if (distanceSq < nearestDistanceSq)
                nearestDistanceSq = distanceSq;
        }

        return nearestDistanceSq == float.MaxValue ? float.MaxValue : MathF.Sqrt(nearestDistanceSq);
    }

    private static List<SliceIsland> FindIslands(
        SliceBitmap bitmap,
        float bedWidthMm,
        float bedDepthMm,
        float sliceHeightMm,
        float supportMergeDistanceMm,
        float minIslandAreaMm2)
    {
        var visited = new bool[bitmap.Pixels.Length];
        var islands = new List<SliceIsland>();

        for (var row = 0; row < bitmap.Height; row++)
        {
            for (var column = 0; column < bitmap.Width; column++)
            {
                var index = (row * bitmap.Width) + column;
                if (visited[index] || bitmap.Pixels[index] == 0)
                    continue;

                var queue = new Queue<(int X, int Y)>();
                var pixels = new List<(int X, int Y)>();
                var boundaryPoints = new List<(float X, float Z)>();
                var sumX = 0f;
                var sumZ = 0f;
                var minColumn = column;
                var maxColumn = column;
                var minRow = row;
                var maxRow = row;

                visited[index] = true;
                queue.Enqueue((column, row));

                while (queue.Count > 0)
                {
                    var current = queue.Dequeue();
                    pixels.Add(current);
                    minColumn = Math.Min(minColumn, current.X);
                    maxColumn = Math.Max(maxColumn, current.X);
                    minRow = Math.Min(minRow, current.Y);
                    maxRow = Math.Max(maxRow, current.Y);
                    var xMm = ColumnToX(current.X, bedWidthMm, bitmap.Width);
                    var zMm = RowToZ(current.Y, bedDepthMm, bitmap.Height);
                    sumX += xMm;
                    sumZ += zMm;

                    var isBoundary = false;
                    foreach (var (nx, ny) in EnumerateNeighbors(current.X, current.Y))
                    {
                        if (nx < 0 || ny < 0 || nx >= bitmap.Width || ny >= bitmap.Height)
                        {
                            isBoundary = true;
                            continue;
                        }

                        var neighborIndex = (ny * bitmap.Width) + nx;
                        if (bitmap.Pixels[neighborIndex] == 0)
                        {
                            isBoundary = true;
                            continue;
                        }

                        if (visited[neighborIndex])
                            continue;

                        visited[neighborIndex] = true;
                        queue.Enqueue((nx, ny));
                    }

                    if (isBoundary)
                        boundaryPoints.Add((xMm, zMm));
                }

                var centroidX = sumX / pixels.Count;
                var centroidZ = sumZ / pixels.Count;
                var pixelWidthMm = bedWidthMm / bitmap.Width;
                var pixelDepthMm = bedDepthMm / bitmap.Height;
                var pixelAreaMm2 = pixelWidthMm * pixelDepthMm;
                var islandAreaMm2 = pixels.Count * pixelAreaMm2;
                var footprintAreaMm2 = (maxColumn - minColumn + 2) * pixelWidthMm * (maxRow - minRow + 2) * pixelDepthMm;
                var effectiveAreaMm2 = MathF.Max(islandAreaMm2, footprintAreaMm2);
                if (effectiveAreaMm2 < minIslandAreaMm2)
                    continue;

                var maxRadiusMm = 0f;
                var mappedPixels = new List<SlicePixel>(pixels.Count);
                foreach (var pixel in pixels)
                {
                    var xMm = ColumnToX(pixel.X, bedWidthMm, bitmap.Width);
                    var zMm = RowToZ(pixel.Y, bedDepthMm, bitmap.Height);
                    mappedPixels.Add(new SlicePixel(xMm, zMm));
                    var dx = xMm - centroidX;
                    var dz = zMm - centroidZ;
                    maxRadiusMm = MathF.Max(maxRadiusMm, MathF.Sqrt((dx * dx) + (dz * dz)) + (MathF.Sqrt(pixelAreaMm2) * 0.5f));
                }

                islands.Add(new SliceIsland(
                    mappedPixels,
                    boundaryPoints,
                    centroidX,
                    centroidZ,
                    sliceHeightMm,
                    pixelAreaMm2,
                    maxRadiusMm));
            }
        }

        return islands;
    }

    private static IEnumerable<(int X, int Y)> EnumerateNeighbors(int x, int y)
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

    private AutoSupportTuning ResolveTuning()
    {
        var config = appConfigService?.GetAsync().GetAwaiter().GetResult();
        return new AutoSupportTuning(
            BedMarginMm: config?.AutoSupportBedMarginMm ?? AppConfigService.DefaultAutoSupportBedMarginMm,
            MinVoxelSizeMm: config?.AutoSupportMinVoxelSizeMm ?? AppConfigService.DefaultAutoSupportMinVoxelSizeMm,
            MaxVoxelSizeMm: config?.AutoSupportMaxVoxelSizeMm ?? AppConfigService.DefaultAutoSupportMaxVoxelSizeMm,
            MinLayerHeightMm: config?.AutoSupportMinLayerHeightMm ?? AppConfigService.DefaultAutoSupportMinLayerHeightMm,
            MaxLayerHeightMm: config?.AutoSupportMaxLayerHeightMm ?? AppConfigService.DefaultAutoSupportMaxLayerHeightMm,
            SupportMergeDistanceMm: config?.AutoSupportMergeDistanceMm ?? AppConfigService.DefaultAutoSupportMergeDistanceMm,
            MinIslandAreaMm2: config?.AutoSupportMinIslandAreaMm2 ?? AppConfigService.DefaultAutoSupportMinIslandAreaMm2,
            MaxSupportDistanceMm: config?.AutoSupportMaxSupportDistanceMm ?? AppConfigService.DefaultAutoSupportMaxSupportDistanceMm,
            ResinStrength: config?.AutoSupportResinStrength ?? AppConfigService.DefaultAutoSupportResinStrength,
            ResinDensityGPerMl: config?.AutoSupportResinDensityGPerMl ?? AppConfigService.DefaultAutoSupportResinDensityGPerMl,
            PeelForceMultiplier: config?.AutoSupportPeelForceMultiplier ?? AppConfigService.DefaultAutoSupportPeelForceMultiplier,
            MicroTipRadiusMm: config?.AutoSupportMicroTipRadiusMm ?? AppConfigService.DefaultAutoSupportMicroTipRadiusMm,
            LightTipRadiusMm: config?.AutoSupportLightTipRadiusMm ?? AppConfigService.DefaultAutoSupportLightTipRadiusMm,
            MediumTipRadiusMm: config?.AutoSupportMediumTipRadiusMm ?? AppConfigService.DefaultAutoSupportMediumTipRadiusMm,
            HeavyTipRadiusMm: config?.AutoSupportHeavyTipRadiusMm ?? AppConfigService.DefaultAutoSupportHeavyTipRadiusMm,
            MaxSupportsPerIsland: config?.AutoSupportMaxSupportsPerIsland ?? AppConfigService.DefaultAutoSupportMaxSupportsPerIsland);
    }

    private sealed record AutoSupportTuning(
        float BedMarginMm,
        float MinVoxelSizeMm,
        float MaxVoxelSizeMm,
        float MinLayerHeightMm,
        float MaxLayerHeightMm,
        float SupportMergeDistanceMm,
        float MinIslandAreaMm2,
        float MaxSupportDistanceMm,
        float ResinStrength,
        float ResinDensityGPerMl,
        float PeelForceMultiplier,
        float MicroTipRadiusMm,
        float LightTipRadiusMm,
        float MediumTipRadiusMm,
        float HeavyTipRadiusMm,
        int MaxSupportsPerIsland);

    private static float GetTipRadiusMm(SupportSize size, AutoSupportTuning tuning) => size switch
    {
        SupportSize.Micro => tuning.MicroTipRadiusMm,
        SupportSize.Light => tuning.LightTipRadiusMm,
        SupportSize.Medium => tuning.MediumTipRadiusMm,
        SupportSize.Heavy => tuning.HeavyTipRadiusMm,
        _ => tuning.MediumTipRadiusMm,
    };

    private static float ComputeMaxPullForce(SupportSize size, AutoSupportTuning tuning)
    {
        var r = GetTipRadiusMm(size, tuning);
        return MathF.PI * r * r * tuning.ResinStrength;
    }

    private static SupportSize NextLargerSize(SupportSize size) => size switch
    {
        SupportSize.Micro => SupportSize.Light,
        SupportSize.Light => SupportSize.Medium,
        SupportSize.Medium => SupportSize.Heavy,
        _ => SupportSize.Heavy,
    };

    private static bool IsNearBase(float sliceHeightMm, float modelHeightMm)
        => sliceHeightMm <= MathF.Max(modelHeightMm * 0.15f, 3f);

    private static int CountNearbySupports(
        Vec3 position,
        List<SupportPoint> supportPoints,
        float radiusMm,
        int excludeIndex)
    {
        var count = 0;
        var radiusSq = radiusMm * radiusMm;
        for (var i = 0; i < supportPoints.Count; i++)
        {
            if (i == excludeIndex) continue;
            var dx = supportPoints[i].Position.X - position.X;
            var dz = supportPoints[i].Position.Z - position.Z;
            if ((dx * dx) + (dz * dz) <= radiusSq)
                count++;
        }
        return count;
    }

    private sealed record SlicePixel(float XMm, float ZMm);

    private sealed record SliceIsland(
        List<SlicePixel> Pixels,
        List<(float X, float Z)> BoundaryPoints,
        float CentroidX,
        float CentroidZ,
        float SliceHeightMm,
        float PixelAreaMm2,
        float IslandRadiusMm);

    private sealed record AdditionalSupportCandidate(float X, float Z, float DistanceMm);

    private sealed record LayerForces(float CumulativeWeightN, float PeelForceN);

    private sealed class SupportAccumulator
    {
        public int Count { get; set; }
        public float SumX { get; set; }
        public float SumZ { get; set; }
        public float SumDistance { get; set; }
    }

    private sealed record PullForceEstimate(int SupportIndex, float Score, Vec3 Vector);
}

public enum SupportSize { Micro, Light, Medium, Heavy }

public sealed record SupportPoint(Vec3 Position, float RadiusMm, Vec3 PullForce, SupportSize Size);

public sealed record SupportPreviewResult(
    IReadOnlyList<SupportPoint> SupportPoints,
    LoadedGeometry SupportGeometry);
