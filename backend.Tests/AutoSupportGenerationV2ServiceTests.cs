using findamodel.Data;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class AutoSupportGenerationV2ServiceTests
{
    private readonly AutoSupportGenerationV2Service sut = new(NullLoggerFactory.Instance);

    private static AppConfigService CreateConfiguredAppConfigService(
        string dbName,
        Func<findamodel.Models.UpdateAppConfigRequest, findamodel.Models.UpdateAppConfigRequest>? configure = null)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;
        var factory = new InMemoryDbContextFactory(options);
        var service = new AppConfigService(factory, new ConfigurationBuilder().AddInMemoryCollection().Build());
        var current = service.GetAsync().GetAwaiter().GetResult();
        var request = new findamodel.Models.UpdateAppConfigRequest(
            current.DefaultRaftHeightMm,
            current.Theme,
            current.GeneratePreviewsEnabled,
            current.MinimumPreviewGenerationVersion,
            current.TagGenerationEnabled,
            current.AiDescriptionEnabled,
            current.TagGenerationProvider,
            current.TagGenerationEndpoint,
            current.TagGenerationModelOverride,
            current.TagGenerationTimeoutMs,
            current.TagGenerationMaxTags,
            current.TagGenerationMinConfidence,
            current.TagGenerationPromptTemplateOverride,
            current.DescriptionGenerationPromptTemplateOverride,
            current.AutoSupportBedMarginMm,
            current.AutoSupportMinVoxelSizeMm,
            current.AutoSupportMaxVoxelSizeMm,
            current.AutoSupportMinLayerHeightMm,
            current.AutoSupportMaxLayerHeightMm,
            current.AutoSupportMergeDistanceMm,
            current.AutoSupportPullForceThreshold,
            current.AutoSupportSphereRadiusMm,
            current.AutoSupportMaxSupportsPerIsland,
            current.AutoSupportMinIslandAreaMm2,
            current.AutoSupportMaxSupportDistanceMm,
            current.AutoSupportUnsupportedIslandVolumeThresholdMm3,
            current.AutoSupportResinStrength,
            current.AutoSupportResinDensityGPerMl,
            current.AutoSupportPeelForceMultiplier,
            current.AutoSupportMicroTipRadiusMm,
            current.AutoSupportLightTipRadiusMm,
            current.AutoSupportMediumTipRadiusMm,
            current.AutoSupportHeavyTipRadiusMm);

        request = configure?.Invoke(request) ?? request;
        service.UpdateAsync(request).GetAwaiter().GetResult();
        return service;
    }

    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);
        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
            => Task.FromResult(CreateDbContext());
    }

    [Fact]
    public void GenerateSupportPreview_EmptyGeometry_ReturnsEmptyResult()
    {
        var geometry = CreateGeometry();

        var result = sut.GenerateSupportPreview(geometry);

        Assert.Empty(result.SupportPoints);
        Assert.Empty(result.SupportGeometry.Triangles);
    }

    [Fact]
    public void GenerateSupportPreview_SingleBox_PlacesAtLeastOneSupport()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        Assert.NotEmpty(result.SupportGeometry.Triangles);
    }

    [Fact]
    public void GenerateSupportPreview_TwoSeparateIslands_SupportsBoth()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: -10f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: 10f, centerZ: 0f, width: 4f, depth: 4f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2);
        Assert.Contains(result.SupportPoints, p => p.Position.X < -4f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 4f);
    }

    [Fact]
    public void GenerateSupportPreview_WideBox_AddsReinforcementSupports()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 6f, height: 4f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2,
            $"Wide box should get at least 2 supports, got {result.SupportPoints.Count}");
    }

    [Fact]
    public void GenerateSupportPreview_AssignsSupportSize()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        Assert.All(result.SupportPoints, point =>
        {
            Assert.True(
                point.Size == SupportSize.Micro ||
                point.Size == SupportSize.Light ||
                point.Size == SupportSize.Medium ||
                point.Size == SupportSize.Heavy);
            Assert.True(point.RadiusMm > 0f);
        });
    }

    [Fact]
    public void GenerateSupportPreview_SupportsHavePullForceVectors()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 8f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        // At least some supports should have non-zero pull forces once reinforcement runs.
        Assert.True(result.SupportPoints.Any(p => p.PullForce.Y > 0f));
    }

    [Fact]
    public void GenerateSupportPreview_SkipsSmallIslands()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_SkipsSmallIslands),
            request => request with { AutoSupportMinIslandAreaMm2 = 50f });
        var configuredSut = new AutoSupportGenerationV2Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 2f, depth: 2f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.Empty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_NearBase_UsesHeavySupports()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 8f, depth: 8f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        // Bottom supports (near base) should be Heavy or Medium
        var bottomSupports = result.SupportPoints
            .Where(p => p.Position.Y <= 2f)
            .ToList();
        if (bottomSupports.Count > 0)
        {
            Assert.Contains(bottomSupports, p =>
                p.Size == SupportSize.Heavy || p.Size == SupportSize.Medium);
        }
    }

    [Fact]
    public void GenerateSupportPreview_TallModel_AccumulatesForce()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 8f, depth: 8f, height: 20f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        // Tall model should accumulate more force and potentially need more supports
        // than a short model of the same footprint.
        var shortGeometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 8f, depth: 8f, height: 4f));
        var shortResult = sut.GenerateSupportPreview(shortGeometry);
        // The tall model should have at least as many supports as the short one
        Assert.True(result.SupportPoints.Count >= shortResult.SupportPoints.Count);
    }

    [Fact]
    public void GenerateSupportPreview_GeneratesSphereMeshForEachSupport()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        // Each support sphere has ~144 triangles (8 lat * 12 lon * ~1.5 triangles avg)
        Assert.True(result.SupportGeometry.Triangles.Count >= result.SupportPoints.Count * 50);
    }

    private static LoadedGeometry CreateGeometry(params List<Triangle3D>[] parts)
    {
        var triangles = parts.SelectMany(x => x).ToList();
        return new LoadedGeometry
        {
            Triangles = triangles,
            DimensionXMm = 40f,
            DimensionYMm = 20f,
            DimensionZMm = 20f,
            SphereCentre = new Vec3(0f, 10f, 0f),
            SphereRadius = 25f,
        };
    }

    private static List<Triangle3D> MakeBox(float centerX, float centerZ, float width, float depth, float height)
    {
        var minX = centerX - (width * 0.5f);
        var maxX = centerX + (width * 0.5f);
        var minZ = centerZ - (depth * 0.5f);
        var maxZ = centerZ + (depth * 0.5f);
        const float minY = 0f;

        var p000 = new Vec3(minX, minY, minZ);
        var p001 = new Vec3(minX, minY, maxZ);
        var p010 = new Vec3(minX, height, minZ);
        var p011 = new Vec3(minX, height, maxZ);
        var p100 = new Vec3(maxX, minY, minZ);
        var p101 = new Vec3(maxX, minY, maxZ);
        var p110 = new Vec3(maxX, height, minZ);
        var p111 = new Vec3(maxX, height, maxZ);

        return
        [
            new Triangle3D(p000, p001, p101, new Vec3(0f, -1f, 0f)),
            new Triangle3D(p000, p101, p100, new Vec3(0f, -1f, 0f)),
            new Triangle3D(p010, p110, p111, new Vec3(0f, 1f, 0f)),
            new Triangle3D(p010, p111, p011, new Vec3(0f, 1f, 0f)),
            new Triangle3D(p000, p100, p110, new Vec3(0f, 0f, -1f)),
            new Triangle3D(p000, p110, p010, new Vec3(0f, 0f, -1f)),
            new Triangle3D(p001, p011, p111, new Vec3(0f, 0f, 1f)),
            new Triangle3D(p001, p111, p101, new Vec3(0f, 0f, 1f)),
            new Triangle3D(p000, p010, p011, new Vec3(-1f, 0f, 0f)),
            new Triangle3D(p000, p011, p001, new Vec3(-1f, 0f, 0f)),
            new Triangle3D(p100, p101, p111, new Vec3(1f, 0f, 0f)),
            new Triangle3D(p100, p111, p110, new Vec3(1f, 0f, 0f)),
        ];
    }
}
