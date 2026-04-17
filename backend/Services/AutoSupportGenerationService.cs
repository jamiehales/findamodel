using Microsoft.Extensions.Logging;

namespace findamodel.Services;

public sealed class AutoSupportGenerationService(ILoggerFactory loggerFactory)
{
    private const float MinUpwardNormalY = 0.5f;
    private const float SupportColumnRadiusMm = 0.35f;
    private const float MaxUnsupportedSpanMm = 12f;
    private readonly ILogger logger = loggerFactory.CreateLogger<AutoSupportGenerationService>();

    public SupportPreviewResult GenerateSupportPreview(LoadedGeometry geometry)
    {
        ArgumentNullException.ThrowIfNull(geometry);

        var islands = BuildTopSurfaceIslands(geometry.Triangles);
        if (islands.Count == 0)
        {
            islands.Add(new TopSurfaceIsland(
                MinX: -geometry.DimensionXMm * 0.5f,
                MaxX: geometry.DimensionXMm * 0.5f,
                MinZ: -geometry.DimensionZMm * 0.5f,
                MaxZ: geometry.DimensionZMm * 0.5f,
                SurfaceY: geometry.DimensionYMm));
        }

        var supportPoints = new List<SupportPoint>();
        foreach (var island in islands)
            AddSupportGrid(supportPoints, island);

        var supportTriangles = new List<Triangle3D>(supportPoints.Count * 12);
        foreach (var point in supportPoints)
            AppendColumn(supportTriangles, point.Position);

        logger.LogDebug("Generated {SupportCount} support preview points across {IslandCount} top-surface islands.", supportPoints.Count, islands.Count);

        return new SupportPreviewResult(
            supportPoints,
            new LoadedGeometry
            {
                Triangles = supportTriangles,
                DimensionXMm = geometry.DimensionXMm,
                DimensionYMm = geometry.DimensionYMm,
                DimensionZMm = geometry.DimensionZMm,
                SphereCentre = geometry.SphereCentre,
                SphereRadius = geometry.SphereRadius,
            });
    }

    private static List<TopSurfaceIsland> BuildTopSurfaceIslands(IReadOnlyList<Triangle3D> triangles)
    {
        var islands = new List<TopSurfaceIsland>();

        foreach (var triangle in triangles)
        {
            if (triangle.Normal.Y < MinUpwardNormalY)
                continue;

            var minX = MathF.Min(triangle.V0.X, MathF.Min(triangle.V1.X, triangle.V2.X));
            var maxX = MathF.Max(triangle.V0.X, MathF.Max(triangle.V1.X, triangle.V2.X));
            var minZ = MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z));
            var maxZ = MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z));
            var surfaceY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));

            var merged = false;
            for (var i = 0; i < islands.Count; i++)
            {
                if (!Overlaps(islands[i], minX, maxX, minZ, maxZ, surfaceY))
                    continue;

                islands[i] = islands[i] with
                {
                    MinX = MathF.Min(islands[i].MinX, minX),
                    MaxX = MathF.Max(islands[i].MaxX, maxX),
                    MinZ = MathF.Min(islands[i].MinZ, minZ),
                    MaxZ = MathF.Max(islands[i].MaxZ, maxZ),
                    SurfaceY = MathF.Max(islands[i].SurfaceY, surfaceY),
                };
                merged = true;
                break;
            }

            if (!merged)
                islands.Add(new TopSurfaceIsland(minX, maxX, minZ, maxZ, surfaceY));
        }

        return islands;
    }

    private static bool Overlaps(TopSurfaceIsland island, float minX, float maxX, float minZ, float maxZ, float surfaceY)
    {
        const float epsilon = 0.01f;
        return MathF.Abs(island.SurfaceY - surfaceY) <= 0.5f
            && minX <= island.MaxX + epsilon
            && maxX >= island.MinX - epsilon
            && minZ <= island.MaxZ + epsilon
            && maxZ >= island.MinZ - epsilon;
    }

    private static void AddSupportGrid(List<SupportPoint> supportPoints, TopSurfaceIsland island)
    {
        var spanX = MathF.Max(0.5f, island.MaxX - island.MinX);
        var spanZ = MathF.Max(0.5f, island.MaxZ - island.MinZ);
        var columns = Math.Max(1, (int)MathF.Ceiling(spanX / MaxUnsupportedSpanMm));
        var rows = Math.Max(1, (int)MathF.Ceiling(spanZ / MaxUnsupportedSpanMm));

        for (var row = 0; row < rows; row++)
        {
            var z = rows == 1
                ? (island.MinZ + island.MaxZ) * 0.5f
                : island.MinZ + (spanZ * row / (rows - 1));

            for (var column = 0; column < columns; column++)
            {
                var x = columns == 1
                    ? (island.MinX + island.MaxX) * 0.5f
                    : island.MinX + (spanX * column / (columns - 1));

                supportPoints.Add(new SupportPoint(new Vec3(x, island.SurfaceY, z)));
            }
        }
    }

    private static void AppendColumn(List<Triangle3D> triangles, Vec3 top)
    {
        var min = new Vec3(top.X - SupportColumnRadiusMm, 0f, top.Z - SupportColumnRadiusMm);
        var max = new Vec3(top.X + SupportColumnRadiusMm, top.Y, top.Z + SupportColumnRadiusMm);

        var p000 = new Vec3(min.X, min.Y, min.Z);
        var p001 = new Vec3(min.X, min.Y, max.Z);
        var p010 = new Vec3(min.X, max.Y, min.Z);
        var p011 = new Vec3(min.X, max.Y, max.Z);
        var p100 = new Vec3(max.X, min.Y, min.Z);
        var p101 = new Vec3(max.X, min.Y, max.Z);
        var p110 = new Vec3(max.X, max.Y, min.Z);
        var p111 = new Vec3(max.X, max.Y, max.Z);

        triangles.Add(new Triangle3D(p000, p001, p101, new Vec3(0f, -1f, 0f)));
        triangles.Add(new Triangle3D(p000, p101, p100, new Vec3(0f, -1f, 0f)));
        triangles.Add(new Triangle3D(p010, p110, p111, new Vec3(0f, 1f, 0f)));
        triangles.Add(new Triangle3D(p010, p111, p011, new Vec3(0f, 1f, 0f)));
        triangles.Add(new Triangle3D(p000, p100, p110, new Vec3(0f, 0f, -1f)));
        triangles.Add(new Triangle3D(p000, p110, p010, new Vec3(0f, 0f, -1f)));
        triangles.Add(new Triangle3D(p001, p011, p111, new Vec3(0f, 0f, 1f)));
        triangles.Add(new Triangle3D(p001, p111, p101, new Vec3(0f, 0f, 1f)));
        triangles.Add(new Triangle3D(p000, p010, p011, new Vec3(-1f, 0f, 0f)));
        triangles.Add(new Triangle3D(p000, p011, p001, new Vec3(-1f, 0f, 0f)));
        triangles.Add(new Triangle3D(p100, p101, p111, new Vec3(1f, 0f, 0f)));
        triangles.Add(new Triangle3D(p100, p111, p110, new Vec3(1f, 0f, 0f)));
    }

    private sealed record TopSurfaceIsland(float MinX, float MaxX, float MinZ, float MaxZ, float SurfaceY);
}

public sealed record SupportPoint(Vec3 Position);

public sealed record SupportPreviewResult(
    IReadOnlyList<SupportPoint> SupportPoints,
    LoadedGeometry SupportGeometry);
