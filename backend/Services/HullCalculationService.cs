using System.Globalization;
using System.Text;
using NetTopologySuite.Geometries;

namespace findamodel.Services;

public class HullCalculationService(
    ModelLoaderService loaderService,
    ILogger<HullCalculationService> logger)
{
    private static readonly GeometryFactory Factory = new();

    public const float RaftOffset = 2f; // 2mm offset for raft/brim removal, to be configurable later

    /// <summary>
    /// Calculates convex, concave (alpha), and convex-sans-raft hulls from pre-loaded geometry. Preferred overload.
    /// Projects vertices onto the X-Z plane (bird's eye view with Y-up coordinate system).
    /// Returns tuple of (convex hull JSON, concave hull JSON, convex sans-raft hull JSON) as [[x,z],[x,z],...] arrays.
    /// The sans-raft hull excludes vertices with Y &lt; 2mm before projection, removing raft/brim geometry.
    /// </summary>
    public Task<(string? ConvexHull, string? ConcaveHull, string? ConvexSansRaftHull)> CalculateHullsAsync(LoadedGeometry geometry)
    {
        try
        {
            var allVertices = geometry.Triangles.SelectMany(t => new[] { t.V0, t.V1, t.V2 });

            // Extract unique X-Z coordinates from triangle vertices.
            // float → double widening is intentional: NTS uses double precision internally.
            var points2D = allVertices
                .Select(v => new Coordinate((double)v.X, (double)v.Z))
                .Distinct()
                .ToArray();

            if (points2D.Length < 3)
            {
                logger.LogWarning("Geometry has fewer than 3 unique X-Z points, skipping hull calculation");
                return Task.FromResult<(string?, string?, string?)>((null, null, null));
            }

            var convexCoords  = CalculateConvexHull(points2D);
            var concaveCoords = new Coordinate[0]; //CalculateConcaveHull(points2D);

            // Sans-raft: exclude vertices at or below defined raft offset (Y-up, 1 unit = 1mm)
            var sansRaftPoints2D = allVertices
                .Where(v => v.Y >= RaftOffset)
                .Select(v => new Coordinate((double)v.X, (double)v.Z))
                .Distinct()
                .ToArray();

            var convexSansRaftCoords = sansRaftPoints2D.Length >= 3
                ? CalculateConvexHull(sansRaftPoints2D)
                : null;

            logger.LogInformation(
                "Hull calculation complete: {ConvexCount} convex vertices, {ConcaveCount} concave vertices, {SansRaftCount} sans-raft vertices",
                convexCoords.Length, concaveCoords.Length, convexSansRaftCoords?.Length ?? 0);

            return Task.FromResult<(string?, string?, string?)>((
                ConvertCoordinatesToJson(convexCoords),
                ConvertCoordinatesToJson(concaveCoords),
                convexSansRaftCoords is not null ? ConvertCoordinatesToJson(convexSansRaftCoords) : null));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull from geometry");
            return Task.FromResult<(string?, string?, string?)>((null, null, null));
        }
    }

    /// <summary>
    /// Loads the file via ModelLoaderService then calculates hulls.
    /// </summary>
    public async Task<(string? ConvexHull, string? ConcaveHull, string? ConvexSansRaftHull)> CalculateHullsAsync(string filePath, string fileType)
    {
        try
        {
            var geometry = await loaderService.LoadModelAsync(filePath, fileType);
            if (geometry is null)
            {
                logger.LogWarning("Model {FilePath} could not be loaded, skipping hull calculation", filePath);
                return (null, null, null);
            }

            return await CalculateHullsAsync(geometry);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull for {FilePath}", filePath);
            return (null, null, null);
        }
    }

    private Coordinate[] CalculateConvexHull(Coordinate[] points)
    {
        var multiPoint = Factory.CreateMultiPointFromCoords(points);
        var convexHull = multiPoint.ConvexHull();
        return convexHull.Coordinates;
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
}
