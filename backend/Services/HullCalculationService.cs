using System.Globalization;
using System.Text;
using NetTopologySuite.Geometries;
using NetTopologySuite.Algorithm.Hull;

namespace findamodel.Services;

public class HullCalculationService(
    ModelLoaderService loaderService,
    ILogger<HullCalculationService> logger)
{
    private static readonly GeometryFactory Factory = new();

    /// <summary>
    /// Calculates convex and concave (alpha) hulls from pre-loaded geometry. Preferred overload.
    /// Projects vertices onto the X-Z plane (bird's eye view with Y-up coordinate system).
    /// Returns tuple of (convex hull JSON, concave hull JSON) as [[x,z],[x,z],...] arrays.
    /// </summary>
    public Task<(string? ConvexHull, string? ConcaveHull)> CalculateHullsAsync(LoadedGeometry geometry)
    {
        try
        {
            // Extract unique X-Z coordinates from triangle vertices.
            // float → double widening is intentional: NTS uses double precision internally.
            var points2D = geometry.Triangles
                .SelectMany(t => new[] { t.V0, t.V1, t.V2 })
                .Select(v => new Coordinate((double)v.X, (double)v.Z))
                .Distinct()
                .ToArray();

            if (points2D.Length < 3)
            {
                logger.LogWarning("Geometry has fewer than 3 unique X-Z points, skipping hull calculation");
                return Task.FromResult<(string?, string?)>((null, null));
            }

            var convexCoords  = CalculateConvexHull(points2D);
            var concaveCoords = CalculateConcaveHull(points2D);

            logger.LogInformation(
                "Hull calculation complete: {ConvexCount} convex vertices, {ConcaveCount} concave vertices",
                convexCoords.Length, concaveCoords.Length);

            return Task.FromResult<(string?, string?)>((
                ConvertCoordinatesToJson(convexCoords),
                ConvertCoordinatesToJson(concaveCoords)));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull from geometry");
            return Task.FromResult<(string?, string?)>((null, null));
        }
    }

    /// <summary>
    /// Loads the file via ModelLoaderService then calculates hulls.
    /// </summary>
    public async Task<(string? ConvexHull, string? ConcaveHull)> CalculateHullsAsync(string filePath, string fileType)
    {
        try
        {
            var geometry = await loaderService.LoadModelAsync(filePath, fileType);
            if (geometry is null)
            {
                logger.LogWarning("Model {FilePath} could not be loaded, skipping hull calculation", filePath);
                return (null, null);
            }

            var points2D = geometry.Triangles
                .SelectMany(t => new[] { t.V0, t.V1, t.V2 })
                .Select(v => new Coordinate((double)v.X, (double)v.Z))
                .Distinct()
                .ToArray();

            if (points2D.Length < 3)
            {
                logger.LogWarning("Model {FilePath} has fewer than 3 unique X-Z points, skipping hull calculation", filePath);
                return (null, null);
            }

            var convexCoords  = CalculateConvexHull(points2D);
            var concaveCoords = CalculateConcaveHull(points2D);

            logger.LogInformation(
                "Hull calculation complete for {FilePath}: {ConvexCount} convex vertices, {ConcaveCount} concave vertices",
                filePath, convexCoords.Length, concaveCoords.Length);

            return (ConvertCoordinatesToJson(convexCoords), ConvertCoordinatesToJson(concaveCoords));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull for {FilePath}", filePath);
            return (null, null);
        }
    }

    private Coordinate[] CalculateConvexHull(Coordinate[] points)
    {
        var multiPoint = Factory.CreateMultiPointFromCoords(points);
        var convexHull = multiPoint.ConvexHull();
        return convexHull.Coordinates;
    }

    private Coordinate[] CalculateConcaveHull(Coordinate[] points)
    {
        try
        {
            var multiPoint = Factory.CreateMultiPointFromCoords(points);
            var concaveHull = new ConcaveHull(multiPoint).GetHull();
            var coords = concaveHull.Coordinates;

            // Validate: if concave hull has too many points relative to input, fall back to convex
            if (coords.Length > points.Length * 0.5)
            {
                logger.LogInformation("Concave hull has too many points ({Count}), using convex hull instead", coords.Length);
                return CalculateConvexHull(points);
            }

            return coords;
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to calculate concave hull, falling back to convex hull");
            return CalculateConvexHull(points);
        }
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
