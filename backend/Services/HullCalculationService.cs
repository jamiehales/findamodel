using System.Globalization;
using System.Text;
using NetTopologySuite.Geometries;
using NetTopologySuite.Algorithm.Hull;

namespace findamodel.Services;

public class HullCalculationService(ILogger<HullCalculationService> logger)
{
    private static readonly GeometryFactory Factory = new();

    /// <summary>
    /// Calculates convex and concave (alpha) hulls for a 3D model.
    /// Projects vertices onto the X-Z plane (bird's eye view with Y-up coordinate system).
    /// Returns tuple of (convex hull JSON, concave hull JSON) as [[x,z],[x,z],...] arrays.
    /// </summary>
    public async Task<(string? ConvexHull, string? ConcaveHull)> CalculateHullsAsync(string filePath, string fileType)
    {
        try
        {
            var vertices = await ParseVerticesAsync(filePath, fileType);
            if (vertices.Count < 3)
            {
                logger.LogWarning("Model {FilePath} has fewer than 3 vertices, skipping hull calculation", filePath);
                return (null, null);
            }

            // Project onto X-Z plane (bird's eye, Y-up: drop Y coordinate)
            var points2D = vertices
                .Select(v => new Coordinate(v.X, v.Z))
                .Distinct()
                .ToArray();

            if (points2D.Length < 3)
            {
                logger.LogWarning("Model {FilePath} has fewer than 3 unique X-Z points, skipping hull calculation", filePath);
                return (null, null);
            }

            // Calculate convex hull
            var convexHullCoords = CalculateConvexHull(points2D);
            var convexHullJson = ConvertCoordinatesToJson(convexHullCoords);

            // Calculate concave hull (alpha shape with automatic alpha detection)
            var concaveHullCoords = CalculateConcaveHull(points2D);
            var concaveHullJson = ConvertCoordinatesToJson(concaveHullCoords);

            logger.LogInformation(
                "Hull calculation complete for {FilePath}: {ConvexCount} convex vertices, {ConcaveCount} concave vertices",
                filePath, convexHullCoords.Length, concaveHullCoords.Length);

            return (convexHullJson, concaveHullJson);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Error calculating hull for {FilePath}", filePath);
            return (null, null);
        }
    }

    private async Task<List<(double X, double Y, double Z)>> ParseVerticesAsync(string filePath, string fileType)
    {
        var vertices = new List<(double X, double Y, double Z)>();

        if (fileType.Equals("stl", StringComparison.OrdinalIgnoreCase))
        {
            vertices = await ParseStlAsync(filePath);
        }
        else if (fileType.Equals("obj", StringComparison.OrdinalIgnoreCase))
        {
            vertices = await ParseObjAsync(filePath);
        }

        return vertices;
    }

    private async Task<List<(double X, double Y, double Z)>> ParseStlAsync(string filePath)
    {
        var vertices = new List<(double X, double Y, double Z)>();

        // Check if binary or ASCII
        var isBinary = await IsBinaryStlAsync(filePath);

        if (isBinary)
        {
            vertices = ParseBinaryStl(filePath);
        }
        else
        {
            vertices = await ParseAsciiStlAsync(filePath);
        }

        return vertices;
    }

    private async Task<bool> IsBinaryStlAsync(string filePath)
    {
        using var stream = new StreamReader(filePath, Encoding.ASCII);
        var firstLine = await stream.ReadLineAsync();
        return firstLine == null || !firstLine.StartsWith("solid");
    }

    private List<(double X, double Y, double Z)> ParseBinaryStl(string filePath)
    {
        var vertices = new List<(double X, double Y, double Z)>();

        using var stream = File.OpenRead(filePath);
        using var reader = new BinaryReader(stream);

        // Skip 80-byte header
        reader.ReadBytes(80);
        uint triangleCount = reader.ReadUInt32();

        for (uint i = 0; i < triangleCount; i++)
        {
            // Skip normal vector (3 floats = 12 bytes)
            reader.ReadBytes(12);

            // Read 3 vertices (3 floats each = 12 bytes per vertex)
            for (int j = 0; j < 3; j++)
            {
                float x = reader.ReadSingle();
                float y = reader.ReadSingle();
                float z = reader.ReadSingle();
                var transformed = TransformToYUp((x, y, z));
                vertices.Add(transformed);
            }

            // Skip attribute byte count
            reader.ReadBytes(2);
        }

        return vertices;
    }

    private async Task<List<(double X, double Y, double Z)>> ParseAsciiStlAsync(string filePath)
    {
        var vertices = new List<(double X, double Y, double Z)>();

        using var stream = new StreamReader(filePath);
        string? line;
        while ((line = await stream.ReadLineAsync()) != null)
        {
            line = line.Trim();
            if (line.StartsWith("vertex"))
            {
                var parts = line.Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length == 4
                    && double.TryParse(parts[1], NumberStyles.Float, CultureInfo.InvariantCulture, out var x)
                    && double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var y)
                    && double.TryParse(parts[3], NumberStyles.Float, CultureInfo.InvariantCulture, out var z))
                {
                    var transformed = TransformToYUp((x, y, z));
                    vertices.Add(transformed);
                }
            }
        }

        return vertices;
    }

    private async Task<List<(double X, double Y, double Z)>> ParseObjAsync(string filePath)
    {
        var vertices = new List<(double X, double Y, double Z)>();

        using var stream = new StreamReader(filePath);
        string? line;
        while ((line = await stream.ReadLineAsync()) != null)
        {
            line = line.Trim();
            if (line.StartsWith("v "))
            {
                var parts = line[2..].Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length >= 3
                    && double.TryParse(parts[0], NumberStyles.Float, CultureInfo.InvariantCulture, out var x)
                    && double.TryParse(parts[1], NumberStyles.Float, CultureInfo.InvariantCulture, out var y)
                    && double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var z))
                {
                    var transformed = TransformToYUp((x, y, z));
                    vertices.Add(transformed);
                }
            }
        }

        return vertices;
    }

    private Coordinate[] CalculateConvexHull(Coordinate[] points)
    {
        var multiPoint = Factory.CreateMultiPointFromCoords(points);
        var convexHull = multiPoint.ConvexHull();
        return convexHull.Coordinates;
    }

    /// <summary>
    /// Transform vertex from Z-up to Y-up coordinate system.
    /// Applies rotateX(π/2) followed by rotateZ(π).
    /// </summary>
    private static (double X, double Y, double Z) TransformToYUp((double X, double Y, double Z) v)
    {
        // Step 1: rotateX(π/2) - rotate around X axis by 90 degrees
        // [x, y, z] -> [x, -z, y]
        var afterRotX = (X: v.X, Y: -v.Z, Z: v.Y);

        // Step 2: rotateZ(π) - rotate around Z axis by 180 degrees
        // [x, y, z] -> [-x, -y, z]
        var afterRotZ = (X: -afterRotX.X, Y: -afterRotX.Y, Z: afterRotX.Z);

        return afterRotZ;
    }

    private Coordinate[] CalculateConcaveHull(Coordinate[] points)
    {
        try
        {
            // Use ConcaveHull with automatic alpha calculation (default)
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
