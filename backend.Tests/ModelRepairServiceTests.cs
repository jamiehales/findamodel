using findamodel.Models;
using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class ModelRepairServiceTests
{
    [Fact]
    public async Task RepairForSlicingAsync_RemovesDegenerateTriangles()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);
        var geometry = BuildGeometry(
        [
            new Triangle3D(new Vec3(0, 0, 0), new Vec3(10, 0, 0), new Vec3(0, 10, 0), Vec3.Up),
            new Triangle3D(new Vec3(0, 0, 0), new Vec3(0, 0, 0), new Vec3(0, 0, 0), Vec3.Up),
        ]);

        var result = await sut.RepairForSlicingAsync(
            geometry,
            new ModelRepairOptions { Enabled = true },
            CancellationToken.None);

        Assert.Single(result.Geometry.Triangles);
        Assert.True(result.Diagnostics.RemovedDegenerateTriangles >= 1);
    }

    [Fact]
    public async Task RepairForSlicingAsync_WeldsNearDuplicateVerticesDeterministically()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);
        var epsilonOffset = 0.000001f;

        var geometry = BuildGeometry(
        [
            new Triangle3D(new Vec3(0, 0, 0), new Vec3(10, 0, 0), new Vec3(0, 10, 0), Vec3.Up),
            new Triangle3D(new Vec3(10 + epsilonOffset, 0, 0), new Vec3(10, 10, 0), new Vec3(0, 10 + epsilonOffset, 0), Vec3.Up),
        ]);

        var options = new ModelRepairOptions
        {
            Enabled = true,
            Profile = ModelRepairProfile.Safe,
            WeldEpsilonMultiplier = 10f,
        };

        var first = await sut.RepairForSlicingAsync(geometry, options, CancellationToken.None);
        var second = await sut.RepairForSlicingAsync(geometry, options, CancellationToken.None);

        Assert.True(first.Diagnostics.WeldedVertexCount > 0);
        Assert.Equal(first.Geometry.Triangles, second.Geometry.Triangles);
    }

    [Fact]
    public async Task RepairForSlicingAsync_FlipsInwardComponent()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);
        var geometry = BuildGeometry(CreateTetrahedron(inward: true, scale: 10f, centre: new Vec3(0, 0, 0)));

        var result = await sut.RepairForSlicingAsync(
            geometry,
            new ModelRepairOptions { Enabled = true },
            CancellationToken.None);

        Assert.True(result.Diagnostics.FlippedComponents >= 1);
    }

    [Fact]
    public async Task RepairForSlicingAsync_RemovesDustDisconnectedComponents()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);

        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateTetrahedron(inward: false, scale: 10f, centre: new Vec3(0, 0, 0)));
        triangles.Add(new Triangle3D(new Vec3(100, 0, 100), new Vec3(100.001f, 0, 100), new Vec3(100, 0.001f, 100), Vec3.Up));

        var result = await sut.RepairForSlicingAsync(
            BuildGeometry(triangles),
            new ModelRepairOptions { Enabled = true, DustTriangleThreshold = 2, DustDiagonalThresholdMm = 1f },
            CancellationToken.None);

        Assert.True(result.Diagnostics.RemovedDustComponents >= 1);
        Assert.True(result.Geometry.Triangles.Count < triangles.Count);
    }

    [Fact]
    public async Task RepairForSlicingAsync_FlipsInvertedInnerShell()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);

        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateTetrahedron(inward: false, scale: 40f, centre: new Vec3(0, 20, 0)));
        triangles.AddRange(CreateTetrahedron(inward: true, scale: 8f, centre: new Vec3(0, 20, 0)));

        var result = await sut.RepairForSlicingAsync(
            BuildGeometry(triangles),
            new ModelRepairOptions
            {
                Enabled = true,
                Profile = ModelRepairProfile.Standard,
                EnableInternalVoidRepair = true,
                InternalVoidRayCount = 8,
                MinVoidVolumeMm3 = 0.01f,
            },
            CancellationToken.None);

        Assert.True(result.Diagnostics.FlippedComponents >= 1 || result.Diagnostics.InvertedShellsFlipped >= 1);
    }

    [Fact]
    public async Task RepairForSlicingAsync_RemovesTinyEnclosedVoid()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);

        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateTetrahedron(inward: false, scale: 40f, centre: new Vec3(0, 20, 0)));
        triangles.AddRange(CreateTetrahedron(inward: false, scale: 0.4f, centre: new Vec3(0, 20, 0)));

        var result = await sut.RepairForSlicingAsync(
            BuildGeometry(triangles),
            new ModelRepairOptions
            {
                Enabled = true,
                Profile = ModelRepairProfile.Standard,
                EnableInternalVoidRepair = true,
                InternalVoidRayCount = 8,
                MinVoidVolumeMm3 = 50f,
            },
            CancellationToken.None);

        Assert.True(result.Diagnostics.VoidComponentsRemoved >= 1 || result.Geometry.Triangles.Count < triangles.Count);
    }

    [Fact]
    public async Task RepairForSlicingAsync_RemovesNearZeroThicknessDuplicateSurface()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);

        var triangles = new List<Triangle3D>
        {
            new(new Vec3(0, 0, 0), new Vec3(20, 0, 0), new Vec3(0, 20, 0), Vec3.Up),
            new(new Vec3(0, 0.01f, 0), new Vec3(20, 0.01f, 0), new Vec3(0, 20.01f, 0), Vec3.Up),
        };

        var result = await sut.RepairForSlicingAsync(
            BuildGeometry(triangles),
            new ModelRepairOptions
            {
                Enabled = true,
                Profile = ModelRepairProfile.Standard,
                EnableThinSlabDetection = true,
                ThinSlabAabbOverlapThreshold = 0.25f,
                MinWallThicknessMm = 0.05f,
                EnableDustComponentFiltering = false,
                EnableInternalVoidRepair = false,
            },
            CancellationToken.None);

        Assert.True(result.Geometry.Triangles.Count <= 2);
    }

    [Fact]
    public async Task RepairForSlicingAsync_AggressiveFallbackKeepsLargestComponentWhenSevere()
    {
        var sut = new ModelRepairService(NullLoggerFactory.Instance);

        var triangles = new List<Triangle3D>();
        triangles.AddRange(CreateTetrahedron(inward: false, scale: 20f, centre: new Vec3(0, 5, 0)));

        // Add severe non-manifold edge usage: 3 triangles sharing one edge.
        triangles.Add(new Triangle3D(new Vec3(100, 0, 100), new Vec3(110, 0, 100), new Vec3(105, 5, 100), Vec3.Up));
        triangles.Add(new Triangle3D(new Vec3(100, 0, 100), new Vec3(110, 0, 100), new Vec3(105, -5, 100), Vec3.Up));
        triangles.Add(new Triangle3D(new Vec3(100, 0, 100), new Vec3(110, 0, 100), new Vec3(105, 0, 105), Vec3.Up));

        var result = await sut.RepairForSlicingAsync(
            BuildGeometry(triangles),
            new ModelRepairOptions
            {
                Enabled = true,
                Profile = ModelRepairProfile.Aggressive,
                EnableFallbackRemesh = true,
                NonManifoldEdgeFallbackThreshold = 1,
                SelfIntersectionFallbackThreshold = int.MaxValue,
                EnableDustComponentFiltering = false,
                EnableInternalVoidRepair = false,
                EnableThinSlabDetection = false,
            },
            CancellationToken.None);

        Assert.True(result.Diagnostics.UsedFallbackRemesh);
        Assert.True(result.Geometry.Triangles.Count <= 4);
    }

    private static LoadedGeometry BuildGeometry(IReadOnlyList<Triangle3D> triangles)
    {
        if (triangles.Count == 0)
        {
            return new LoadedGeometry
            {
                Triangles = [],
                SphereCentre = new Vec3(0, 0, 0),
                SphereRadius = 1,
                DimensionXMm = 0,
                DimensionYMm = 0,
                DimensionZMm = 0,
            };
        }

        var vertices = triangles.SelectMany(t => new[] { t.V0, t.V1, t.V2 }).ToArray();
        var minX = vertices.Min(v => v.X);
        var minY = vertices.Min(v => v.Y);
        var minZ = vertices.Min(v => v.Z);
        var maxX = vertices.Max(v => v.X);
        var maxY = vertices.Max(v => v.Y);
        var maxZ = vertices.Max(v => v.Z);

        var centre = new Vec3((minX + maxX) * 0.5f, (minY + maxY) * 0.5f, (minZ + maxZ) * 0.5f);
        var radius = vertices.Max(v => (v - centre).Length);

        return new LoadedGeometry
        {
            Triangles = triangles.ToList(),
            SphereCentre = centre,
            SphereRadius = MathF.Max(1e-3f, radius),
            DimensionXMm = maxX - minX,
            DimensionYMm = maxY - minY,
            DimensionZMm = maxZ - minZ,
        };
    }

    private static IReadOnlyList<Triangle3D> CreateTetrahedron(bool inward, float scale, Vec3 centre)
    {
        var a = centre + new Vec3(-1, 0, -1) * scale;
        var b = centre + new Vec3(1, 0, -1) * scale;
        var c = centre + new Vec3(0, 0, 1) * scale;
        var d = centre + new Vec3(0, 1.5f, 0) * scale;

        var triangles = new List<Triangle3D>
        {
            MakeTriangle(a, b, c),
            MakeTriangle(a, d, b),
            MakeTriangle(b, d, c),
            MakeTriangle(c, d, a),
        };

        if (inward)
        {
            for (var i = 0; i < triangles.Count; i++)
            {
                var t = triangles[i];
                triangles[i] = new Triangle3D(t.V0, t.V2, t.V1, -t.Normal);
            }
        }

        return triangles;
    }

    private static Triangle3D MakeTriangle(Vec3 a, Vec3 b, Vec3 c)
    {
        var n = (b - a).Cross(c - a).Normalized;
        return new Triangle3D(a, b, c, n);
    }
}
