using System.Globalization;
using System.Text;
using NetTopologySuite.Algorithm.Hull;
using NetTopologySuite.Geometries;
using NetTopologySuite.Simplify;

namespace findamodel.Services;

public class HullCalculationService(
    ModelLoaderService loaderService,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Hull);
    private static readonly GeometryFactory Factory = new();
    public const int DefaultMaxHullVertices = 20;
    public const int CurrentHullGenerationVersion = 3;
    private const int MaxConcaveHullInputPoints = 30000;
    private const double MinConcaveRatio = 0.08;
    private const double MaxConcaveRatio = 0.95;

    public const float DefaultRaftHeightMm = 2f;

    /// <summary>
    /// Calculates convex, concave (alpha), and convex-sans-raft hulls from pre-loaded geometry. Preferred overload.
    /// Projects vertices onto the X-Z plane (bird's eye view with Y-up coordinate system).
    /// Returns tuple of (convex hull JSON, concave hull JSON, convex sans-raft hull JSON) as [[x,z],[x,z],...] arrays.
    /// The sans-raft hull excludes vertices with Y below the configured raft height before projection,
    /// removing raft/brim geometry.
    /// </summary>
    public Task<(string? ConvexHull, string? ConcaveHull, string? ConvexSansRaftHull)> CalculateHullsAsync(
        LoadedGeometry geometry,
        int maxHullVertices = DefaultMaxHullVertices,
        float raftHeightMm = DefaultRaftHeightMm)
    {
        try
        {
            var allVertices = geometry.Triangles.SelectMany(t => new[] { t.V0, t.V1, t.V2 }).ToArray();

            // Extract unique X-Z coordinates from triangle vertices.
            // float → double widening is intentional: NTS uses double precision internally.
            var points2D = allVertices
                .Select(v => new Coordinate((double)v.X, (double)v.Z))
                .Distinct(new CoordinateComparer(1e-6))
                .ToArray();

            if (points2D.Length < 3)
            {
                logger.LogWarning("Geometry has fewer than 3 unique X-Z points, skipping hull calculation");
                return Task.FromResult<(string?, string?, string?)>((null, null, null));
            }

            var convexCoords = CalculateEnclosingConvexHull(points2D, maxHullVertices);
            var concaveCoords = CalculateConcaveHull(points2D, maxHullVertices);

            // Sans-raft: exclude vertices at or below configured raft cutoff (Y-up, 1 unit = 1mm)
            var sansRaftPoints2D = allVertices
                .Where(v => v.Y >= raftHeightMm)
                .Select(v => new Coordinate((double)v.X, (double)v.Z))
                .Distinct(new CoordinateComparer(1e-6))
                .ToArray();

            var convexSansRaftCoords = sansRaftPoints2D.Length >= 3
                ? CalculateEnclosingConvexHull(sansRaftPoints2D, maxHullVertices)
                : null;

            logger.LogInformation(
                "Hull calculation complete: {ConvexCount} convex vertices, {ConcaveCount} concave vertices, {SansRaftCount} sans-raft vertices (max {MaxHullVertices})",
                convexCoords?.Length ?? 0, concaveCoords?.Length ?? 0, convexSansRaftCoords?.Length ?? 0, maxHullVertices);

            return Task.FromResult<(string?, string?, string?)>((
                convexCoords is not null ? ConvertCoordinatesToJson(convexCoords) : null,
                concaveCoords is not null ? ConvertCoordinatesToJson(concaveCoords) : null,
                convexSansRaftCoords is not null ? ConvertCoordinatesToJson(convexSansRaftCoords) : null));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull from geometry");
            return Task.FromResult<(string?, string?, string?)>((null, null, null));
        }
    }

    private Coordinate[]? CalculateConcaveHull(Coordinate[] points, int maxVertices)
    {
        if (points.Length < 3) return null;

        var reducedPoints = DownsampleForConcaveHull(points, MaxConcaveHullInputPoints, 1e-6);
        var convexRing = NormalizeRing(Factory.CreateMultiPointFromCoords(points).ConvexHull().Coordinates);
        var reducedWithBoundary = reducedPoints
            .Concat(convexRing)
            .Distinct(new CoordinateComparer(1e-6))
            .ToArray();

        var multiPoint = Factory.CreateMultiPointFromCoords(reducedWithBoundary);
        var convexArea = Math.Max(Math.Abs(SignedArea(convexRing)), 1e-9);

        var initialRatio = EstimateInitialConcaveRatio(reducedPoints.Length, maxVertices);
        var candidateRatios = new[]
        {
            initialRatio,
            Math.Clamp(initialRatio + 0.08, MinConcaveRatio, MaxConcaveRatio),
            Math.Clamp(initialRatio + 0.18, MinConcaveRatio, MaxConcaveRatio),
            Math.Clamp(initialRatio + 0.35, MinConcaveRatio, MaxConcaveRatio),
            MaxConcaveRatio,
        }
        .Distinct()
        .ToArray();

        Coordinate[]? best = null;
        double bestAreaRatio = double.NegativeInfinity;

        foreach (var ratio in candidateRatios)
        {
            Geometry hullGeometry;
            try
            {
                hullGeometry = ConcaveHull.ConcaveHullByLengthRatio(multiPoint, ratio, isHolesAllowed: false);
            }
            catch (Exception ex)
            {
                logger.LogDebug(ex, "Concave hull attempt failed at ratio {Ratio}", ratio);
                continue;
            }

            var ring = ExtractLargestPolygonShell(hullGeometry);
            if (ring is null || ring.Length < 3) continue;

            if (ring.Length > maxVertices)
                ring = SimplifyRingToVertexBudget(ring, maxVertices);

            if (ring.Length < 3 || !IsValidRing(ring))
                continue;

            var ringArea = Math.Abs(SignedArea(ring));
            var areaRatio = ringArea / convexArea;

            // Reject unstable shapes that collapse too far inside the outer envelope.
            if (areaRatio < 0.60)
                continue;

            if (areaRatio > bestAreaRatio)
            {
                bestAreaRatio = areaRatio;
                best = ring;
            }
        }

        return best;
    }

    private static bool IsValidRing(Coordinate[] ring)
    {
        if (ring.Length < 3)
            return false;

        var closed = CloseRing(ring);
        var linearRing = Factory.CreateLinearRing(closed);
        if (!linearRing.IsSimple || !linearRing.IsValid)
            return false;

        var polygon = Factory.CreatePolygon(linearRing);
        return polygon.IsValid && polygon.Area > 0;
    }

    /// <summary>
    /// Loads the file via ModelLoaderService then calculates hulls.
    /// </summary>
    public async Task<(string? ConvexHull, string? ConcaveHull, string? ConvexSansRaftHull)> CalculateHullsAsync(
        string filePath,
        string fileType,
        int maxHullVertices = DefaultMaxHullVertices,
        float raftHeightMm = DefaultRaftHeightMm)
    {
        try
        {
            var geometry = await loaderService.LoadModelAsync(filePath, fileType);
            if (geometry is null)
            {
                logger.LogWarning("Model {FilePath} could not be loaded, skipping hull calculation", filePath);
                return (null, null, null);
            }

            return await CalculateHullsAsync(geometry, maxHullVertices, raftHeightMm);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull for {FilePath}", filePath);
            return (null, null, null);
        }
    }

    private Coordinate[]? CalculateEnclosingConvexHull(Coordinate[] points, int maxVertices)
    {
        if (points.Length < 3) return null;

        var multiPoint = Factory.CreateMultiPointFromCoords(points);
        var baseCoords = NormalizeRing(multiPoint.ConvexHull().Coordinates);
        if (baseCoords.Length < 3) return null;

        if (baseCoords.Length <= maxVertices)
            return baseCoords;

        Coordinate[]? best = null;
        double bestArea = double.PositiveInfinity;
        const int phaseSamples = 16;
        const double epsilon = 1e-5;

        for (int s = 0; s < phaseSamples; s++)
        {
            var phase = (Math.PI * 2.0 * s) / (phaseSamples * maxVertices);
            var candidate = BuildSupportPolygon(baseCoords, maxVertices, phase, epsilon);
            if (candidate == null || candidate.Length < 3) continue;
            if (!ContainsAllPoints(candidate, points, epsilon * 4)) continue;

            var area = Math.Abs(SignedArea(candidate));
            if (area < bestArea)
            {
                bestArea = area;
                best = candidate;
            }
        }

        if (best is not null)
            return best;

        return BuildBoundingRectangle(points);
    }

    private static Coordinate[] NormalizeRing(Coordinate[] ring)
    {
        if (ring.Length == 0) return [];

        var end = ring.Length;
        if (ring.Length > 1 && ring[0].Equals2D(ring[^1]))
            end -= 1;

        var normalized = new List<Coordinate>(end);
        for (var i = 0; i < end; i++)
        {
            if (normalized.Count == 0 || !normalized[^1].Equals2D(ring[i]))
                normalized.Add(new Coordinate(ring[i].X, ring[i].Y));
        }

        if (normalized.Count >= 3 && SignedArea(normalized) < 0)
            normalized.Reverse();

        return normalized.ToArray();
    }

    private static Coordinate[]? BuildSupportPolygon(Coordinate[] hull, int maxVertices, double phase, double epsilon)
    {
        var normals = new (double X, double Y, double H)[maxVertices];
        for (var i = 0; i < maxVertices; i++)
        {
            var theta = phase + (Math.PI * 2.0 * i / maxVertices);
            var nx = Math.Cos(theta);
            var ny = Math.Sin(theta);
            var h = double.NegativeInfinity;
            for (var p = 0; p < hull.Length; p++)
            {
                var d = nx * hull[p].X + ny * hull[p].Y;
                if (d > h) h = d;
            }
            normals[i] = (nx, ny, h + epsilon);
        }

        var vertices = new Coordinate[maxVertices];
        for (var i = 0; i < maxVertices; i++)
        {
            var a = normals[i];
            var b = normals[(i + 1) % maxVertices];
            var det = a.X * b.Y - a.Y * b.X;
            if (Math.Abs(det) < 1e-10) return null;

            var x = (a.H * b.Y - b.H * a.Y) / det;
            var y = (a.X * b.H - b.X * a.H) / det;
            if (double.IsNaN(x) || double.IsNaN(y) || double.IsInfinity(x) || double.IsInfinity(y)) return null;
            vertices[i] = new Coordinate(x, y);
        }

        return NormalizeRing(vertices);
    }

    private static Coordinate[] BuildBoundingRectangle(Coordinate[] points)
    {
        var minX = points.Min(p => p.X);
        var maxX = points.Max(p => p.X);
        var minY = points.Min(p => p.Y);
        var maxY = points.Max(p => p.Y);

        return
        [
            new Coordinate(minX, minY),
            new Coordinate(maxX, minY),
            new Coordinate(maxX, maxY),
            new Coordinate(minX, maxY),
        ];
    }

    private static Coordinate[] DownsampleForConcaveHull(Coordinate[] points, int maxPoints, double epsilon)
    {
        if (points.Length <= maxPoints)
            return points;

        var minX = points.Min(p => p.X);
        var maxX = points.Max(p => p.X);
        var minY = points.Min(p => p.Y);
        var maxY = points.Max(p => p.Y);

        var width = Math.Max(maxX - minX, epsilon);
        var height = Math.Max(maxY - minY, epsilon);
        var area = Math.Max(width * height, epsilon);
        var cellSize = Math.Max(Math.Sqrt(area / maxPoints), epsilon);

        var sampled = new Dictionary<(long X, long Y), Coordinate>();
        foreach (var p in points)
        {
            var gx = (long)Math.Floor((p.X - minX) / cellSize);
            var gy = (long)Math.Floor((p.Y - minY) / cellSize);
            var key = (gx, gy);
            if (!sampled.ContainsKey(key))
                sampled[key] = p;
        }

        return sampled.Values.ToArray();
    }

    private static double EstimateInitialConcaveRatio(int pointCount, int maxVertices)
    {
        if (pointCount <= 0 || maxVertices <= 0) return 0.22;

        var density = Math.Clamp((double)maxVertices / pointCount, 0.0, 1.0);
        var ratio = 0.16 + (0.55 * density);
        return Math.Clamp(ratio, MinConcaveRatio, 0.45);
    }

    private Coordinate[]? ExtractLargestPolygonShell(Geometry geometry)
    {
        switch (geometry)
        {
            case Polygon polygon:
                {
                    var shell = NormalizeRing(polygon.ExteriorRing.Coordinates);
                    return shell.Length >= 3 ? shell : null;
                }
            case MultiPolygon multiPolygon:
                {
                    Polygon? largest = null;
                    var largestArea = double.NegativeInfinity;
                    for (var i = 0; i < multiPolygon.NumGeometries; i++)
                    {
                        if (multiPolygon.GetGeometryN(i) is not Polygon p) continue;
                        var area = p.Area;
                        if (area > largestArea)
                        {
                            largestArea = area;
                            largest = p;
                        }
                    }

                    return largest is not null ? NormalizeRing(largest.ExteriorRing.Coordinates) : null;
                }
            case GeometryCollection collection:
                {
                    Coordinate[]? best = null;
                    var bestArea = double.NegativeInfinity;

                    for (var i = 0; i < collection.NumGeometries; i++)
                    {
                        var candidate = ExtractLargestPolygonShell(collection.GetGeometryN(i));
                        if (candidate is null || candidate.Length < 3) continue;
                        var area = Math.Abs(SignedArea(candidate));
                        if (area > bestArea)
                        {
                            bestArea = area;
                            best = candidate;
                        }
                    }

                    return best;
                }
            default:
                return null;
        }
    }

    private Coordinate[] SimplifyRingToVertexBudget(Coordinate[] ring, int maxVertices)
    {
        if (ring.Length <= maxVertices || maxVertices < 3)
            return ring;

        var polygon = Factory.CreatePolygon(CloseRing(ring));

        var minX = ring.Min(c => c.X);
        var maxX = ring.Max(c => c.X);
        var minY = ring.Min(c => c.Y);
        var maxY = ring.Max(c => c.Y);
        var diagonal = Math.Sqrt(Math.Pow(maxX - minX, 2) + Math.Pow(maxY - minY, 2));
        if (diagonal <= 0)
            return ring;

        Coordinate[] best = ring;
        var low = 0.0;
        var high = diagonal;

        for (var i = 0; i < 18; i++)
        {
            var mid = (low + high) / 2.0;
            var simplified = TopologyPreservingSimplifier.Simplify(polygon, mid);
            var candidate = ExtractLargestPolygonShell(simplified);
            if (candidate is null || candidate.Length < 3)
            {
                low = mid;
                continue;
            }

            if (candidate.Length > maxVertices)
            {
                low = mid;
            }
            else
            {
                best = candidate;
                high = mid;
            }
        }

        return best;
    }

    private static Coordinate[] CloseRing(Coordinate[] ring)
    {
        if (ring.Length == 0)
            return [];
        if (ring[0].Equals2D(ring[^1]))
            return ring;

        var closed = new Coordinate[ring.Length + 1];
        for (var i = 0; i < ring.Length; i++)
            closed[i] = new Coordinate(ring[i].X, ring[i].Y);
        closed[^1] = new Coordinate(ring[0].X, ring[0].Y);
        return closed;
    }

    private static bool ContainsAllPoints(Coordinate[] polygon, Coordinate[] points, double epsilon)
    {
        if (polygon.Length < 3) return false;

        for (var i = 0; i < polygon.Length; i++)
        {
            var a = polygon[i];
            var b = polygon[(i + 1) % polygon.Length];
            var ex = b.X - a.X;
            var ey = b.Y - a.Y;
            for (var j = 0; j < points.Length; j++)
            {
                var px = points[j].X - a.X;
                var py = points[j].Y - a.Y;
                // CCW polygon => right-side cross means outside
                var cross = ex * py - ey * px;
                if (cross < -epsilon) return false;
            }
        }
        return true;
    }

    private static double SignedArea(IReadOnlyList<Coordinate> polygon)
    {
        if (polygon.Count < 3) return 0;
        double area = 0;
        for (var i = 0; i < polygon.Count; i++)
        {
            var a = polygon[i];
            var b = polygon[(i + 1) % polygon.Count];
            area += a.X * b.Y - b.X * a.Y;
        }
        return area / 2.0;
    }

    private string ConvertCoordinatesToJson(Coordinate[] coordinates)
    {
        if (coordinates.Length == 0) return "[]";

        var sb = new StringBuilder("[");
        for (int i = 0; i < coordinates.Length; i++)
        {
            if (i > 0) sb.Append(",");
            sb.Append($"[{coordinates[i].X.ToString(CultureInfo.InvariantCulture)},{coordinates[i].Y.ToString(CultureInfo.InvariantCulture)}]");
        }
        sb.Append("]");

        return sb.ToString();
    }

    private sealed class CoordinateComparer(double epsilon) : IEqualityComparer<Coordinate>
    {
        public bool Equals(Coordinate? a, Coordinate? b)
        {
            if (ReferenceEquals(a, b)) return true;
            if (a is null || b is null) return false;
            return Math.Abs(a.X - b.X) <= epsilon && Math.Abs(a.Y - b.Y) <= epsilon;
        }

        public int GetHashCode(Coordinate c)
        {
            var x = Math.Round(c.X / epsilon);
            var y = Math.Round(c.Y / epsilon);
            return HashCode.Combine(x, y);
        }
    }
}
