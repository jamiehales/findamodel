using findamodel.Data;
using findamodel.Models;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class AutoSupportGenerationV3ServiceTests
{
    private readonly AutoSupportGenerationV3Service sut = new(NullLoggerFactory.Instance);

    private static AppConfigService CreateConfiguredAppConfigService(
        string dbName,
        Func<UpdateAppConfigRequest, UpdateAppConfigRequest>? configure = null)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;
        var factory = new InMemoryDbContextFactory(options);
        var service = new AppConfigService(
            factory,
            new ConfigurationBuilder().AddInMemoryCollection().Build());
        var current = service.GetAsync().GetAwaiter().GetResult();
        var request = new UpdateAppConfigRequest(
            DefaultRaftHeightMm: current.DefaultRaftHeightMm,
            Theme: current.Theme,
            GeneratePreviewsEnabled: current.GeneratePreviewsEnabled,
            MinimumPreviewGenerationVersion: current.MinimumPreviewGenerationVersion,
            TagGenerationEnabled: current.TagGenerationEnabled,
            AiDescriptionEnabled: current.AiDescriptionEnabled,
            TagGenerationProvider: current.TagGenerationProvider,
            TagGenerationEndpoint: current.TagGenerationEndpoint,
            TagGenerationModel: current.TagGenerationModelOverride,
            TagGenerationTimeoutMs: current.TagGenerationTimeoutMs,
            TagGenerationMaxTags: current.TagGenerationMaxTags,
            TagGenerationMinConfidence: current.TagGenerationMinConfidence,
            TagGenerationPromptTemplate: current.TagGenerationPromptTemplateOverride,
            DescriptionGenerationPromptTemplate: current.DescriptionGenerationPromptTemplateOverride,
            AutoSupportBedMarginMm: current.AutoSupportBedMarginMm,
            AutoSupportMinVoxelSizeMm: current.AutoSupportMinVoxelSizeMm,
            AutoSupportMaxVoxelSizeMm: current.AutoSupportMaxVoxelSizeMm,
            AutoSupportMinLayerHeightMm: current.AutoSupportMinLayerHeightMm,
            AutoSupportMaxLayerHeightMm: current.AutoSupportMaxLayerHeightMm,
            AutoSupportMergeDistanceMm: current.AutoSupportMergeDistanceMm,
            AutoSupportPullForceThreshold: current.AutoSupportPullForceThreshold,
            AutoSupportSphereRadiusMm: current.AutoSupportSphereRadiusMm,
            AutoSupportMaxSupportsPerIsland: current.AutoSupportMaxSupportsPerIsland,
            AutoSupportMinIslandAreaMm2: current.AutoSupportMinIslandAreaMm2,
            AutoSupportMaxSupportDistanceMm: current.AutoSupportMaxSupportDistanceMm,
            AutoSupportUnsupportedIslandVolumeThresholdMm3: current.AutoSupportUnsupportedIslandVolumeThresholdMm3,
            AutoSupportResinStrength: current.AutoSupportResinStrength,
            AutoSupportCrushForceThreshold: current.AutoSupportCrushForceThreshold,
            AutoSupportMaxAngularForce: current.AutoSupportMaxAngularForce,
            AutoSupportResinDensityGPerMl: current.AutoSupportResinDensityGPerMl,
            AutoSupportPeelForceMultiplier: current.AutoSupportPeelForceMultiplier,
            AutoSupportMicroTipRadiusMm: current.AutoSupportMicroTipRadiusMm,
            AutoSupportLightTipRadiusMm: current.AutoSupportLightTipRadiusMm,
            AutoSupportMediumTipRadiusMm: current.AutoSupportMediumTipRadiusMm,
            AutoSupportHeavyTipRadiusMm: current.AutoSupportHeavyTipRadiusMm,
            AutoSupportV2VoxelSizeMm: current.AutoSupportV2VoxelSizeMm,
            AutoSupportV2OptimizationEnabled: current.AutoSupportV2OptimizationEnabled,
            AutoSupportV2CoarseVoxelSizeMm: current.AutoSupportV2CoarseVoxelSizeMm,
            AutoSupportV2FineVoxelSizeMm: current.AutoSupportV2FineVoxelSizeMm,
            AutoSupportV2RefinementMarginMm: current.AutoSupportV2RefinementMarginMm,
            AutoSupportV2RefinementMaxRegions: current.AutoSupportV2RefinementMaxRegions,
            AutoSupportV2RiskForceMarginRatio: current.AutoSupportV2RiskForceMarginRatio,
            AutoSupportV2MinRegionVolumeMm3: current.AutoSupportV2MinRegionVolumeMm3);

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
        // A single box should always get support(s) at the overhang start layer.
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_SingleBox_SupportNearBottomOfModel()
    {
        var height = 6f;
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: height));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        // Supports for a box resting on the bed appear at the first layer (near Y=0)
        Assert.All(result.SupportPoints, p => Assert.True(p.Position.Y <= height * 0.2f,
            $"Support Y={p.Position.Y:F2} should be near the bottom (<= {height * 0.2f:F2})"));
    }

    [Fact]
    public void GenerateSupportPreview_TwoSeparateBoxes_PlacesTipOnEach()
    {
        // Two separate boxes at different heights - each should receive a tip support
        var geometry = CreateGeometry(
            MakeBox(centerX: -10f, centerZ: 0f, width: 4f, depth: 4f, height: 4f),
            MakeBox(centerX: 10f, centerZ: 0f, width: 4f, depth: 4f, height: 8f));

        var result = sut.GenerateSupportPreview(geometry);

        // One tip for each disconnected tower
        Assert.True(result.SupportPoints.Count >= 2,
            $"Expected at least 2 tip supports, got {result.SupportPoints.Count}");
        Assert.Contains(result.SupportPoints, p => p.Position.X < -4f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 4f);
    }

    [Fact]
    public void GenerateSupportPreview_SeparateBoxes_BothGetSupports()
    {
        // Two separate towers both starting at Y=0 - each gets a support at their first layer
        var geometry = CreateGeometry(
            MakeBox(centerX: -10f, centerZ: 0f, width: 4f, depth: 4f, height: 4f),
            MakeBox(centerX: 10f, centerZ: 0f, width: 4f, depth: 4f, height: 10f));

        var result = sut.GenerateSupportPreview(geometry);

        // Both towers should receive at least one support each
        Assert.True(result.SupportPoints.Count >= 2);
        Assert.Contains(result.SupportPoints, p => p.Position.X < -4f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 4f);
    }

    [Fact]
    public void GenerateSupportPreview_AssignsValidSupportSizes()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

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
    public void GenerateSupportPreview_IslandsListAlwaysEmpty()
    {
        // Method 3 intentionally omits island outlines from the result
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.Empty(result.Islands);
    }

    [Fact]
    public void GenerateSupportPreview_SupportGeometryContainsSphereTriangles()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        // Each sphere has ~144 triangles (8 lat * 12 lon * ~1.5 avg)
        Assert.True(result.SupportGeometry.Triangles.Count >= result.SupportPoints.Count * 50);
    }

    [Fact]
    public void GenerateSupportPreview_SupportsHavePositiveRadius()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        Assert.All(result.SupportPoints, p => Assert.True(p.RadiusMm > 0f));
    }

    [Fact]
    public void GenerateSupportPreview_WideBox_ReportsNegativeYForceForCompressedSupport()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_WideBox_ReportsNegativeYForceForCompressedSupport),
            request => request with
            {
                AutoSupportMergeDistanceMm = 25f,
                AutoSupportMaxSupportDistanceMm = 30f,
                AutoSupportMaxSupportsPerIsland = 1,
                AutoSupportResinStrength = 1000f,
                AutoSupportCrushForceThreshold = 10000f,
                AutoSupportMaxAngularForce = 10000f,
            });
        var configuredSut = new AutoSupportGenerationV3Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 20f, depth: 6f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        var support = Assert.Single(result.SupportPoints);
        Assert.True(support.PullForce.Y < 0f, $"Expected compressive negative Y force, got {support.PullForce.Y:F3}");
    }

    [Fact]
    public void GenerateSupportPreview_WideBox_AddsSupportsWhenAngularForceExceeded()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_WideBox_AddsSupportsWhenAngularForceExceeded),
            request => request with
            {
                AutoSupportMergeDistanceMm = 25f,
                AutoSupportMaxSupportDistanceMm = 30f,
                AutoSupportMaxSupportsPerIsland = 4,
                AutoSupportResinStrength = 1000f,
                AutoSupportCrushForceThreshold = 10000f,
                AutoSupportMaxAngularForce = 20f,
            });
        var configuredSut = new AutoSupportGenerationV3Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 20f, depth: 6f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2, $"Expected angular force reinforcement, got {result.SupportPoints.Count} supports.");
    }

    [Fact]
    public void GenerateSupportPreview_WideBox_AddsSupportsWhenCrushForceExceeded()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_WideBox_AddsSupportsWhenCrushForceExceeded),
            request => request with
            {
                AutoSupportMergeDistanceMm = 25f,
                AutoSupportMaxSupportDistanceMm = 30f,
                AutoSupportMaxSupportsPerIsland = 4,
                AutoSupportResinStrength = 1000f,
                AutoSupportCrushForceThreshold = 5f,
                AutoSupportMaxAngularForce = 10000f,
            });
        var configuredSut = new AutoSupportGenerationV3Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 20f, depth: 6f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2, $"Expected crush-force reinforcement, got {result.SupportPoints.Count} supports.");
    }

    [Fact]
    public void GenerateSupportPreview_TwoOverlappingBoxes_PlacesTipForEachIsland()
    {
        // Two overlapping boxes that share the same top layer merge into one connected island,
        // so only one tip support should be placed (no merge distance applied)
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f),
            MakeBox(centerX: 0.5f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        // The two overlapping boxes connect at the top layer into one island - one tip expected
        Assert.True(result.SupportPoints.Count >= 1,
            $"Overlapping boxes should produce at least 1 tip support, got {result.SupportPoints.Count}");
    }

    [Fact]
    public void GenerateSupportPreview_SingleBox_BodyGeometryIsVoxelMesh()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotNull(result.BodyGeometry);
        Assert.NotEmpty(result.BodyGeometry.Triangles);
        // Merged voxel meshing should compress a simple box to a very small block mesh.
        Assert.True(result.BodyGeometry.Triangles.Count <= 24,
            $"Expected merged voxel mesh to stay compact, got {result.BodyGeometry.Triangles.Count} triangles.");
    }

    [Fact]
    public void GenerateSupportPreview_EmptyGeometry_BodyGeometryIsNull()
    {
        var geometry = CreateGeometry();

        var result = sut.GenerateSupportPreview(geometry);

        Assert.Null(result.BodyGeometry);
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
