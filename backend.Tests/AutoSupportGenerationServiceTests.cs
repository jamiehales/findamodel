using findamodel.Data;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class AutoSupportGenerationServiceTests
{
    private readonly AutoSupportGenerationService sut = new(NullLoggerFactory.Instance);

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
            current.AutoSupportMaxSupportDistanceMm);

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
    public void GenerateSupportPreview_AddsOneSupportPerInitialIsland()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: -8f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: 8f, centerZ: 0f, width: 4f, depth: 4f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2);
        Assert.Contains(result.SupportPoints, point => point.Position.X < -4f);
        Assert.Contains(result.SupportPoints, point => point.Position.X > 4f);
        Assert.NotEmpty(result.SupportGeometry.Triangles);
    }

    [Fact]
    public void GenerateSupportPreview_AddsExtraSupportsForWidePullForceSpan()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 6f, height: 4f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2);
        Assert.NotEmpty(result.SupportGeometry.Triangles);
    }

    [Fact]
    public void AutoSupportJobDto_StoresSupportPointsWithPullForces()
    {
        var dto = new findamodel.Models.AutoSupportJobDto(
            Guid.NewGuid(),
            "completed",
            100,
            1,
            null,
            [
                new findamodel.Models.AutoSupportPointDto(
                    1f,
                    2f,
                    3f,
                    0.75f,
                    new findamodel.Models.AutoSupportVectorDto(0.5f, 4f, -0.25f)),
            ]);

        var supportPoints = Assert.IsAssignableFrom<IReadOnlyList<findamodel.Models.AutoSupportPointDto>>(dto.SupportPoints);
        var point = Assert.Single(supportPoints);
        Assert.Equal(1f, point.X);
        Assert.Equal(4f, point.PullForce.Y);
    }

    [Fact]
    public void GenerateSupportPreview_SkipsIslandsBelowConfiguredMinimumArea()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_SkipsIslandsBelowConfiguredMinimumArea),
            request => request with { AutoSupportMinIslandAreaMm2 = 20f });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(MakeBox(centerX: 0f, centerZ: 0f, width: 2f, depth: 2f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.Empty(result.SupportPoints);
        Assert.Empty(result.SupportGeometry.Triangles);
    }

    [Fact]
    public void GenerateSupportPreview_RespectsConfiguredMaximumSupportDistance()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_RespectsConfiguredMaximumSupportDistance),
            request => request with
            {
                AutoSupportMergeDistanceMm = 2f,
                AutoSupportMaxSupportDistanceMm = 8f,
                AutoSupportPullForceThreshold = 1f,
                AutoSupportMaxSupportsPerIsland = 10,
            });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(MakeBox(centerX: 0f, centerZ: 0f, width: 36f, depth: 6f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 3);

        var xPositions = result.SupportPoints.Select(point => point.Position.X).OrderBy(x => x).ToArray();
        for (var i = 1; i < xPositions.Length; i++)
            Assert.True(xPositions[i] - xPositions[i - 1] <= 8.5f, $"Support spacing {xPositions[i] - xPositions[i - 1]:F2} exceeded threshold.");
    }

    [Fact]
    public void GenerateSupportPreview_AllowsRequiredSupportForNearbySeparateIsland()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_AllowsRequiredSupportForNearbySeparateIsland),
            request => request with
            {
                AutoSupportMergeDistanceMm = 5f,
                AutoSupportMaxSupportDistanceMm = 6f,
                AutoSupportMaxSupportsPerIsland = 4,
            });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: -2.5f, centerZ: 0f, width: 1.2f, depth: 2f, height: 4f),
            MakeBox(centerX: 2.5f, centerZ: 0f, width: 1.2f, depth: 2f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2);
        Assert.Contains(result.SupportPoints, point => point.Position.X < 0f);
        Assert.Contains(result.SupportPoints, point => point.Position.X > 0f);
    }

    private static LoadedGeometry CreateGeometry(params List<Triangle3D>[] parts)
    {
        var triangles = parts.SelectMany(x => x).ToList();
        return new LoadedGeometry
        {
            Triangles = triangles,
            DimensionXMm = 40f,
            DimensionYMm = 10f,
            DimensionZMm = 20f,
            SphereCentre = new Vec3(0f, 5f, 0f),
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
