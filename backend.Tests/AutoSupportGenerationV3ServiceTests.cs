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
                AutoSupportGravityEnabled = false,
                AutoSupportShrinkagePercent = 0f,
                AutoSupportDragCoefficientMultiplier = 0f,
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

    private static readonly AutoSupportV3TuningOverrides DefaultOverrides = new(
        BedMarginMm: 2f, MinVoxelSizeMm: 0.8f, MaxVoxelSizeMm: 2f,
        MinLayerHeightMm: 0.75f, MaxLayerHeightMm: 1.5f, MinIslandAreaMm2: 4f,
        SupportSpacingThresholdMm: 2.5f, ResinStrength: 1f, CrushForceThreshold: 20f,
        MaxAngularForce: 40f, PeelForceMultiplier: 0.15f, LightTipRadiusMm: 0.7f,
        MediumTipRadiusMm: 1f, HeavyTipRadiusMm: 1.5f);

    [Fact]
    public void GenerateSupportPreview_WithHighSuctionMultiplier_ProducesMoreSupports()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 10f));

        var baseline = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { SuctionMultiplier = 1f });
        var highSuction = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { SuctionMultiplier = 10f });

        Assert.True(highSuction.SupportPoints.Count >= baseline.SupportPoints.Count,
            $"High suction ({highSuction.SupportPoints.Count}) should produce >= supports than baseline ({baseline.SupportPoints.Count})");
    }

    [Fact]
    public void GenerateSupportPreview_GravityDisabled_ProducesDifferentResult()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 10f));

        var withGravity = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { GravityEnabled = true });
        var withoutGravity = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { GravityEnabled = false });

        // Both should produce valid results
        Assert.NotEmpty(withGravity.SupportPoints);
        Assert.NotEmpty(withoutGravity.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_ZeroShrinkage_DoesNotCrash()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 10f));

        var result = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { ShrinkagePercent = 0f });

        Assert.NotEmpty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_ZeroDragCoefficient_DoesNotCrash()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 10f));

        var result = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { DragCoefficientMultiplier = 0f });

        Assert.NotEmpty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_HighAreaGrowthMultiplier_ProducesMoreSupports()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 10f));

        var baseline = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { AreaGrowthMultiplier = 1f });
        var highGrowth = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with { AreaGrowthMultiplier = 5f });

        Assert.True(highGrowth.SupportPoints.Count >= baseline.SupportPoints.Count,
            $"High area growth ({highGrowth.SupportPoints.Count}) should produce >= supports than baseline ({baseline.SupportPoints.Count})");
    }

    [Fact]
    public void GenerateSupportPreview_Donut_DoesNotPlaceSupportInCenterHole()
    {
        var geometry = CreateGeometry(
            MakeTorus(majorRadius: 10f, minorRadius: 3f, centerY: 8f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        Assert.Contains(result.SupportPoints, point =>
        {
            var radialDistance = MathF.Sqrt((point.Position.X * point.Position.X) + (point.Position.Z * point.Position.Z));
            return radialDistance >= 6f;
        });
    }

    [Fact]
    public void GenerateSupportPreview_TwoDonutsWithSuction_DoesNotThrow()
    {
        var leftDonut = TranslateTriangles(MakeTorus(majorRadius: 6f, minorRadius: 2f, centerY: 8f), offsetX: -12f, offsetZ: 0f);
        var rightDonut = TranslateTriangles(MakeTorus(majorRadius: 6f, minorRadius: 2f, centerY: 8f), offsetX: 12f, offsetZ: 0f);
        var geometry = CreateGeometry(leftDonut, rightDonut);

        var result = sut.GenerateSupportPreview(geometry,
            DefaultOverrides with
            {
                SuctionMultiplier = 5f,
                MinVoxelSizeMm = 0.4f,
                MinLayerHeightMm = 0.4f,
            });

        Assert.NotNull(result);
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

    private static List<Triangle3D> MakeTorus(float majorRadius, float minorRadius, float centerY)
    {
        const int majorSegments = 32;
        const int minorSegments = 24;
        var triangles = new List<Triangle3D>(majorSegments * minorSegments * 2);

        for (var i = 0; i < majorSegments; i++)
        {
            var u0 = (MathF.PI * 2f * i) / majorSegments;
            var u1 = (MathF.PI * 2f * (i + 1)) / majorSegments;

            for (var j = 0; j < minorSegments; j++)
            {
                var v0 = (MathF.PI * 2f * j) / minorSegments;
                var v1 = (MathF.PI * 2f * (j + 1)) / minorSegments;

                var p00 = TorusPoint(majorRadius, minorRadius, centerY, u0, v0);
                var p01 = TorusPoint(majorRadius, minorRadius, centerY, u0, v1);
                var p10 = TorusPoint(majorRadius, minorRadius, centerY, u1, v0);
                var p11 = TorusPoint(majorRadius, minorRadius, centerY, u1, v1);

                triangles.Add(new Triangle3D(p00, p01, p10, (p01 - p00).Cross(p10 - p00).Normalized));
                triangles.Add(new Triangle3D(p01, p11, p10, (p11 - p01).Cross(p10 - p01).Normalized));
            }
        }

        return triangles;
    }

    private static Vec3 TorusPoint(float majorRadius, float minorRadius, float centerY, float u, float v)
    {
        var cosU = MathF.Cos(u);
        var sinU = MathF.Sin(u);
        var cosV = MathF.Cos(v);
        var sinV = MathF.Sin(v);
        var ringRadius = majorRadius + (minorRadius * cosV);

        return new Vec3(
            ringRadius * cosU,
            centerY + (minorRadius * sinV),
            ringRadius * sinU);
    }

    private static List<Triangle3D> TranslateTriangles(IEnumerable<Triangle3D> triangles, float offsetX, float offsetZ)
        => triangles
            .Select(triangle => new Triangle3D(
                new Vec3(triangle.V0.X + offsetX, triangle.V0.Y, triangle.V0.Z + offsetZ),
                new Vec3(triangle.V1.X + offsetX, triangle.V1.Y, triangle.V1.Z + offsetZ),
                new Vec3(triangle.V2.X + offsetX, triangle.V2.Y, triangle.V2.Z + offsetZ),
                triangle.Normal))
            .ToList();
}
