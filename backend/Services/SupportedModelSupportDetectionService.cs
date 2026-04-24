using findamodel.Models;

namespace findamodel.Services;

public sealed class SupportedModelSupportDetectionService(
    AutoSupportGenerationService autoSupportGenerationService,
    AppConfigService appConfigService,
    ILoggerFactory loggerFactory)
{
    private const float ContactCellSizeMm = 0.35f;
    private const float ClusterHeightToleranceMm = 1.25f;
    private const float MinimumContactRadiusMm = 0.2f;
    private const float MaximumContactSnapDistanceMm = 8f;
    private const float MaximumVerticalOffsetMm = 10f;
    private const int MaximumPatchesToEvaluate = 128;

    private readonly ILogger logger = loggerFactory.CreateLogger<SupportedModelSupportDetectionService>();

    public async Task<SupportPreviewResult> GeneratePreviewAsync(
        LoadedGeometry bodyGeometry,
        LoadedGeometry supportGeometry,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(bodyGeometry);
        ArgumentNullException.ThrowIfNull(supportGeometry);

        cancellationToken.ThrowIfCancellationRequested();

        var bodyPreview = autoSupportGenerationService.GenerateSupportPreview(bodyGeometry);
        if (supportGeometry.Triangles.Count == 0)
        {
            return new SupportPreviewResult(
                [],
                supportGeometry,
                bodyPreview.Islands,
                bodyPreview.SliceLayers,
                bodyPreview.BodyGeometry ?? bodyGeometry);
        }

        var appConfig = await appConfigService.GetAsync();
        var candidatePatches = BuildContactPatches(supportGeometry.Triangles, upwardFacingOnly: true);
        var supportPoints = BuildSupportPointsFromPatches(candidatePatches, bodyGeometry.Triangles, appConfig, cancellationToken);

        if (supportPoints.Count == 0)
        {
            // Fallback for real-world supported meshes where contact caps are not reliably upward-facing.
            candidatePatches = BuildContactPatches(supportGeometry.Triangles, upwardFacingOnly: false);
            supportPoints = BuildSupportPointsFromPatches(candidatePatches, bodyGeometry.Triangles, appConfig, cancellationToken);
        }

        if (supportPoints.Count == 0)
        {
            // Last-resort fallback: promote highest patches directly so support detection never returns empty.
            foreach (var patch in candidatePatches.OrderByDescending(patch => patch.TopY).Take(64))
            {
                var measuredRadiusMm = Math.Max(patch.RadiusMm, MinimumContactRadiusMm);
                var (size, radiusMm) = SnapToNearestSupportSize(measuredRadiusMm, appConfig);
                supportPoints.Add(new SupportPoint(
                    patch.Centroid,
                    radiusMm,
                    new Vec3(0f, 0f, 0f),
                    size));
            }
        }

        logger.LogInformation(
            "Supported-model detection evaluated {PatchCount} patches and produced {SupportCount} support points",
            candidatePatches.Count,
            supportPoints.Count);

        if (supportPoints.Count == 0)
        {
            logger.LogWarning(
                "Supported-model contact detection found support geometry but no mesh contact patches ({SupportTriangleCount} support triangles)",
                supportGeometry.Triangles.Count);
        }

        var deduplicatedSupportPoints = supportPoints
            .OrderByDescending(point => point.Position.Y)
            .ThenBy(point => point.Position.X)
            .ThenBy(point => point.Position.Z)
            .Aggregate(new List<SupportPoint>(), (result, point) =>
            {
                var isDuplicate = result.Any(existing =>
                {
                    var delta = existing.Position - point.Position;
                    var planarDistance = MathF.Sqrt((delta.X * delta.X) + (delta.Z * delta.Z));
                    return planarDistance <= Math.Max(0.5f, MathF.Min(existing.RadiusMm, point.RadiusMm));
                });

                if (!isDuplicate)
                    result.Add(point);

                return result;
            });

        return new SupportPreviewResult(
            deduplicatedSupportPoints,
            supportGeometry,
            bodyPreview.Islands,
            bodyPreview.SliceLayers,
            bodyPreview.BodyGeometry ?? bodyGeometry);
    }

    private List<SupportPoint> BuildSupportPointsFromPatches(
        IReadOnlyList<ContactPatch> candidatePatches,
        IReadOnlyList<Triangle3D> bodyTriangles,
        AppConfigDto appConfig,
        CancellationToken cancellationToken)
    {
        var patchesToEvaluate = candidatePatches
            .OrderByDescending(patch => patch.TopY)
            .ThenByDescending(patch => patch.RadiusMm)
            .Take(MaximumPatchesToEvaluate)
            .ToArray();

        var supportPoints = new List<SupportPoint>(patchesToEvaluate.Length);

        foreach (var patch in patchesToEvaluate)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var approximateContact = new Vec3(patch.Centroid.X, patch.TopY, patch.Centroid.Z);
            var snappedContact = FindClosestPointOnMesh(approximateContact, bodyTriangles);
            var snapDistance = (snappedContact - approximateContact).Length;

            if (snapDistance > Math.Max(MaximumContactSnapDistanceMm, patch.RadiusMm * 3f))
                continue;

            if (MathF.Abs(snappedContact.Y - patch.TopY) > MaximumVerticalOffsetMm)
                continue;

            var measuredRadiusMm = Math.Max(patch.RadiusMm, MinimumContactRadiusMm);
            var (size, radiusMm) = SnapToNearestSupportSize(measuredRadiusMm, appConfig);
            supportPoints.Add(new SupportPoint(
                snappedContact,
                radiusMm,
                new Vec3(0f, 0f, 0f),
                size));
        }

        return supportPoints;
    }

    private static IReadOnlyList<ContactPatch> BuildContactPatches(
        IReadOnlyList<Triangle3D> supportTriangles,
        bool upwardFacingOnly)
    {
        var topTriangleByCell = new Dictionary<(int X, int Z), TriangleSample>();

        foreach (var triangle in supportTriangles)
        {
            if (upwardFacingOnly && triangle.Normal.Y < 0.15f)
                continue;

            var centroid = new Vec3(
                (triangle.V0.X + triangle.V1.X + triangle.V2.X) / 3f,
                (triangle.V0.Y + triangle.V1.Y + triangle.V2.Y) / 3f,
                (triangle.V0.Z + triangle.V1.Z + triangle.V2.Z) / 3f);
            var topY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            AddTriangleSample(new TriangleSample(triangle, centroid, topY));
        }

        if (topTriangleByCell.Count == 0)
            return [];

        var visited = new HashSet<(int X, int Z)>();
        var patches = new List<ContactPatch>();

        foreach (var (cell, sample) in topTriangleByCell.OrderByDescending(pair => pair.Value.TopY))
        {
            if (visited.Contains(cell))
                continue;

            var queue = new Queue<(int X, int Z)>();
            var clusterSamples = new List<TriangleSample>();
            visited.Add(cell);
            queue.Enqueue(cell);

            while (queue.Count > 0)
            {
                var current = queue.Dequeue();
                var currentSample = topTriangleByCell[current];
                clusterSamples.Add(currentSample);

                for (var dz = -1; dz <= 1; dz++)
                {
                    for (var dx = -1; dx <= 1; dx++)
                    {
                        if (dx == 0 && dz == 0)
                            continue;

                        var neighbor = (current.X + dx, current.Z + dz);
                        if (visited.Contains(neighbor) || !topTriangleByCell.TryGetValue(neighbor, out var neighborSample))
                            continue;

                        if (MathF.Abs(neighborSample.TopY - currentSample.TopY) > ClusterHeightToleranceMm)
                            continue;

                        visited.Add(neighbor);
                        queue.Enqueue(neighbor);
                    }
                }
            }

            if (clusterSamples.Count == 0)
                continue;

            var centroidX = clusterSamples.Average(item => item.Centroid.X);
            var centroidZ = clusterSamples.Average(item => item.Centroid.Z);
            var topY = clusterSamples.Max(item => item.TopY);
            var clusterVertices = clusterSamples
                .SelectMany(item => new[] { item.Triangle.V0, item.Triangle.V1, item.Triangle.V2 })
                .ToArray();
            var maxPlanarDistance = clusterVertices.Max(item =>
            {
                var dx = item.X - centroidX;
                var dz = item.Z - centroidZ;
                return MathF.Sqrt((dx * dx) + (dz * dz));
            });

            patches.Add(new ContactPatch(
                new Vec3(centroidX, topY, centroidZ),
                topY,
                Math.Max(maxPlanarDistance + (ContactCellSizeMm * 0.5f), MinimumContactRadiusMm)));
        }

        return patches;

        void AddTriangleSample(TriangleSample sample)
        {
            var key = (
                (int)MathF.Round(sample.Centroid.X / ContactCellSizeMm),
                (int)MathF.Round(sample.Centroid.Z / ContactCellSizeMm));

            if (!topTriangleByCell.TryGetValue(key, out var existing) || sample.TopY > existing.TopY)
                topTriangleByCell[key] = sample;
        }
    }

    private static (SupportSize Size, float RadiusMm) SnapToNearestSupportSize(float measuredRadiusMm, AppConfigDto appConfig)
    {
        var candidates = new (SupportSize Size, float RadiusMm)[]
        {
            (SupportSize.Micro, appConfig.AutoSupportMicroTipRadiusMm),
            (SupportSize.Light, appConfig.AutoSupportLightTipRadiusMm),
            (SupportSize.Medium, appConfig.AutoSupportMediumTipRadiusMm),
            (SupportSize.Heavy, appConfig.AutoSupportHeavyTipRadiusMm),
        };

        var best = candidates[0];
        var bestDistance = MathF.Abs(measuredRadiusMm - best.RadiusMm);

        for (var i = 1; i < candidates.Length; i++)
        {
            var candidate = candidates[i];
            var distance = MathF.Abs(measuredRadiusMm - candidate.RadiusMm);
            if (distance < bestDistance)
            {
                best = candidate;
                bestDistance = distance;
            }
        }

        return best;
    }

    private static Vec3 FindClosestPointOnMesh(Vec3 point, IReadOnlyList<Triangle3D> triangles)
    {
        var bestPoint = point;
        var bestDistanceSq = float.PositiveInfinity;

        foreach (var triangle in triangles)
        {
            var candidate = ClosestPointOnTriangle(point, triangle.V0, triangle.V1, triangle.V2);
            var distanceSq = (candidate - point).LengthSq;
            if (distanceSq < bestDistanceSq)
            {
                bestDistanceSq = distanceSq;
                bestPoint = candidate;
            }
        }

        return bestPoint;
    }

    private static Vec3 ClosestPointOnTriangle(Vec3 point, Vec3 a, Vec3 b, Vec3 c)
    {
        var ab = b - a;
        var ac = c - a;
        var ap = point - a;

        var d1 = ab.Dot(ap);
        var d2 = ac.Dot(ap);
        if (d1 <= 0f && d2 <= 0f)
            return a;

        var bp = point - b;
        var d3 = ab.Dot(bp);
        var d4 = ac.Dot(bp);
        if (d3 >= 0f && d4 <= d3)
            return b;

        var vc = (d1 * d4) - (d3 * d2);
        if (vc <= 0f && d1 >= 0f && d3 <= 0f)
        {
            var v = d1 / (d1 - d3);
            return a + (ab * v);
        }

        var cp = point - c;
        var d5 = ab.Dot(cp);
        var d6 = ac.Dot(cp);
        if (d6 >= 0f && d5 <= d6)
            return c;

        var vb = (d5 * d2) - (d1 * d6);
        if (vb <= 0f && d2 >= 0f && d6 <= 0f)
        {
            var w = d2 / (d2 - d6);
            return a + (ac * w);
        }

        var va = (d3 * d6) - (d5 * d4);
        if (va <= 0f && (d4 - d3) >= 0f && (d5 - d6) >= 0f)
        {
            var edge = c - b;
            var w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
            return b + (edge * w);
        }

        var denominator = 1f / (va + vb + vc);
        var barycentricV = vb * denominator;
        var barycentricW = vc * denominator;
        return a + (ab * barycentricV) + (ac * barycentricW);
    }

    private sealed record ContactPatch(Vec3 Centroid, float TopY, float RadiusMm);

    private sealed record TriangleSample(Triangle3D Triangle, Vec3 Centroid, float TopY);
}