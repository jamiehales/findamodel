using findamodel.Data;
using findamodel.Models;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class SupportedModelSupportDetectionServiceTests
{
    [Fact]
    public async Task GeneratePreviewAsync_ConnectedSupportBase_FindsMultipleContactPoints()
    {
        var appConfigService = CreateAppConfigService(nameof(GeneratePreviewAsync_ConnectedSupportBase_FindsMultipleContactPoints));
        var autoSupportGenerationService = new AutoSupportGenerationService(appConfigService, NullLoggerFactory.Instance);
        var sut = new SupportedModelSupportDetectionService(
            autoSupportGenerationService,
            appConfigService,
            NullLoggerFactory.Instance);

        var bodyGeometry = CreateGeometry(
            MakeBox(-8f, 10f, -2f, -4f, 12f, 2f),
            MakeBox(4f, 10f, -2f, 8f, 12f, 2f));
        var supportGeometry = CreateGeometry(
            MakeBox(-10f, 0f, -3f, 1f, -3f, 3f),
            MakeBox(-7f, 1f, -0.8f, 10f, -0.8f, 0.8f),
            MakeBox(5f, 1f, -0.8f, 10f, -0.8f, 0.8f));

        var result = await sut.GeneratePreviewAsync(bodyGeometry, supportGeometry);

        Assert.True(result.SupportPoints.Count >= 2, $"Expected at least 2 detected contact points, got {result.SupportPoints.Count}.");
        Assert.NotEmpty(result.SliceLayers ?? []);
        Assert.Equal(supportGeometry.Triangles.Count, result.SupportGeometry.Triangles.Count);
        Assert.Contains(result.SupportPoints, point => point.Position.X < -5f && point.Position.Y >= 9.5f);
        Assert.Contains(result.SupportPoints, point => point.Position.X > 4f && point.Position.Y >= 9.5f);
        var allowedRadii = new[] { 0.4f, 0.7f, 1.0f, 1.5f };
        Assert.All(result.SupportPoints, point =>
            Assert.Contains(allowedRadii, allowed => MathF.Abs(point.RadiusMm - allowed) < 0.0001f));
    }

    private static AppConfigService CreateAppConfigService(string dbName)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;

        var factory = new InMemoryDbContextFactory(options);
        return new AppConfigService(factory, new ConfigurationBuilder().AddInMemoryCollection().Build());
    }

    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);

        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken cancellationToken = default)
            => Task.FromResult(CreateDbContext());
    }

    private static LoadedGeometry CreateGeometry(params List<Triangle3D>[] triangleGroups)
    {
        var triangles = triangleGroups.SelectMany(group => group).ToList();
        var vertices = triangles.SelectMany(triangle => new[] { triangle.V0, triangle.V1, triangle.V2 }).ToList();
        var minX = vertices.Min(vertex => vertex.X);
        var maxX = vertices.Max(vertex => vertex.X);
        var minY = vertices.Min(vertex => vertex.Y);
        var maxY = vertices.Max(vertex => vertex.Y);
        var minZ = vertices.Min(vertex => vertex.Z);
        var maxZ = vertices.Max(vertex => vertex.Z);
        var sphereCentre = new Vec3((minX + maxX) * 0.5f, (minY + maxY) * 0.5f, (minZ + maxZ) * 0.5f);
        var sphereRadius = vertices.Max(vertex => (vertex - sphereCentre).Length);

        return new LoadedGeometry
        {
            Triangles = triangles,
            DimensionXMm = maxX - minX,
            DimensionYMm = maxY - minY,
            DimensionZMm = maxZ - minZ,
            SphereCentre = sphereCentre,
            SphereRadius = sphereRadius,
        };
    }

    private static List<Triangle3D> MakeBox(
        float minX,
        float minY,
        float minZ,
        float maxX,
        float maxY,
        float maxZ)
    {
        var p000 = new Vec3(minX, minY, minZ);
        var p001 = new Vec3(minX, minY, maxZ);
        var p010 = new Vec3(minX, maxY, minZ);
        var p011 = new Vec3(minX, maxY, maxZ);
        var p100 = new Vec3(maxX, minY, minZ);
        var p101 = new Vec3(maxX, minY, maxZ);
        var p110 = new Vec3(maxX, maxY, minZ);
        var p111 = new Vec3(maxX, maxY, maxZ);

        return
        [
            new(p000, p100, p110, new Vec3(0f, 0f, -1f)),
            new(p000, p110, p010, new Vec3(0f, 0f, -1f)),
            new(p001, p011, p111, new Vec3(0f, 0f, 1f)),
            new(p001, p111, p101, new Vec3(0f, 0f, 1f)),
            new(p000, p001, p101, new Vec3(0f, -1f, 0f)),
            new(p000, p101, p100, new Vec3(0f, -1f, 0f)),
            new(p010, p110, p111, new Vec3(0f, 1f, 0f)),
            new(p010, p111, p011, new Vec3(0f, 1f, 0f)),
            new(p000, p010, p011, new Vec3(-1f, 0f, 0f)),
            new(p000, p011, p001, new Vec3(-1f, 0f, 0f)),
            new(p100, p101, p111, new Vec3(1f, 0f, 0f)),
            new(p100, p111, p110, new Vec3(1f, 0f, 0f)),
        ];
    }
}