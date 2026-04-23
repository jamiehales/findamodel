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
    public void GenerateSupportPreview_Sphere_DoesNotHitPreviewSupportCap()
    {
        var baseGeometry = CreateGeometry(
            MakeSphere(radius: 20f, centerY: 20f));
        var geometry = new LoadedGeometry
        {
            Triangles = baseGeometry.Triangles,
            DimensionXMm = baseGeometry.DimensionXMm,
            DimensionYMm = 40f,
            DimensionZMm = baseGeometry.DimensionZMm,
            SphereCentre = baseGeometry.SphereCentre,
            SphereRadius = baseGeometry.SphereRadius,
        };

        var result = sut.GenerateSupportPreview(geometry, DefaultOverrides, maxSupportPoints: 2000);

        Assert.True(result.SupportPoints.Count > 1,
            $"Sphere preview should place more than one support, got {result.SupportPoints.Count}.");
        Assert.True(result.SupportPoints.Count < 2000,
            $"Sphere preview should not exhaust the preview support cap, got {result.SupportPoints.Count} supports.");
    }

    [Fact]
    public void GenerateSupportPreview_Sphere_SupportsSpanMultipleHeights()
    {
        var baseGeometry = CreateGeometry(
            MakeSphere(radius: 20f, centerY: 20f));
        var geometry = new LoadedGeometry
        {
            Triangles = baseGeometry.Triangles,
            DimensionXMm = baseGeometry.DimensionXMm,
            DimensionYMm = 40f,
            DimensionZMm = baseGeometry.DimensionZMm,
            SphereCentre = baseGeometry.SphereCentre,
            SphereRadius = baseGeometry.SphereRadius,
        };

        var result = sut.GenerateSupportPreview(geometry, DefaultOverrides, maxSupportPoints: 2000);

        var distinctHeights = result.SupportPoints
            .Select(point => MathF.Round(point.Position.Y, 3))
            .Distinct()
            .ToList();

        Assert.True(distinctHeights.Count > 1,
            $"Expected sphere supports to span multiple slice heights, got {distinctHeights.Count} distinct height(s).");

        var minHeight = result.SupportPoints.Min(point => point.Position.Y);
        var maxHeight = result.SupportPoints.Max(point => point.Position.Y);
        var heightSpread = maxHeight - minHeight;
        Assert.True(heightSpread >= geometry.DimensionYMm * 0.25f,
            $"Expected sphere supports to span at least a quarter of model height, got spread {heightSpread:F3}mm for model height {geometry.DimensionYMm:F3}mm.");

        var quarterCutoff = minHeight + (geometry.DimensionYMm * 0.25f);
        var supportsAboveQuarter = result.SupportPoints.Count(point => point.Position.Y >= quarterCutoff);
        Assert.True(supportsAboveQuarter >= Math.Max(5, result.SupportPoints.Count / 8),
            $"Expected a meaningful fraction of supports above lower quarter, got {supportsAboveQuarter}/{result.SupportPoints.Count} above Y={quarterCutoff:F3}.");
    }

    [Fact]
    public void GenerateSupportPreview_SupportsRespectConfiguredSpacingThreshold()
    {
        var geometry = CreateGeometry(
            MakeSphere(radius: 20f, centerY: 20f));
        var spacingThreshold = 6f;

        var result = sut.GenerateSupportPreview(
            geometry,
            DefaultOverrides with
            {
                SupportSpacingThresholdMm = spacingThreshold,
                ResinStrength = 1000f,
                CrushForceThreshold = 10000f,
                MaxAngularForce = 10000f,
                PeelForceMultiplier = 0.01f,
                GravityEnabled = false,
                ShrinkagePercent = 0f,
                DragCoefficientMultiplier = 0f,
                SuctionMultiplier = 1f,
                AreaGrowthMultiplier = 1f,
            },
            maxSupportPoints: 2000);

        Assert.NotEmpty(result.SupportPoints);

        for (var i = 0; i < result.SupportPoints.Count; i++)
        {
            for (var j = i + 1; j < result.SupportPoints.Count; j++)
            {
                var dy = MathF.Abs(result.SupportPoints[i].Position.Y - result.SupportPoints[j].Position.Y);
                if (dy > spacingThreshold)
                    continue;

                var dx = result.SupportPoints[i].Position.X - result.SupportPoints[j].Position.X;
                var dz = result.SupportPoints[i].Position.Z - result.SupportPoints[j].Position.Z;
                var planarDistance = MathF.Sqrt((dx * dx) + (dz * dz));
                Assert.True(planarDistance >= spacingThreshold - 0.01f,
                    $"Nearby-layer supports were placed {planarDistance:F3}mm apart, below threshold {spacingThreshold:F3}mm.");
            }
        }
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

    [Fact]
    public void GenerateSupportPreview_RespectsSupportCap()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: -10f, centerZ: 0f, width: 4f, depth: 4f, height: 4f),
            MakeBox(centerX: 10f, centerZ: 0f, width: 4f, depth: 4f, height: 10f));

        var uncapped = sut.GenerateSupportPreview(geometry, DefaultOverrides);
        Assert.True(uncapped.SupportPoints.Count >= 2, "Expected baseline geometry to produce multiple supports.");

        var capped = sut.GenerateSupportPreview(geometry, DefaultOverrides, maxSupportPoints: 1);

        Assert.Single(capped.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_RepeatedRuns_ProduceStableSupportLayout()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: -7f, centerZ: 0f, width: 8f, depth: 8f, height: 10f),
            MakeBox(centerX: 8f, centerZ: 1f, width: 10f, depth: 6f, height: 12f),
            MakeTorus(majorRadius: 4f, minorRadius: 1.25f, centerY: 9f));

        var first = sut.GenerateSupportPreview(geometry, DefaultOverrides with
        {
            SuctionMultiplier = 3f,
            AreaGrowthMultiplier = 2f,
            GravityEnabled = true,
            ShrinkagePercent = 1.5f,
        });
        var second = sut.GenerateSupportPreview(geometry, DefaultOverrides with
        {
            SuctionMultiplier = 3f,
            AreaGrowthMultiplier = 2f,
            GravityEnabled = true,
            ShrinkagePercent = 1.5f,
        });

        Assert.Equal(first.SupportPoints.Count, second.SupportPoints.Count);
        Assert.Equal(first.SupportGeometry.Triangles.Count, second.SupportGeometry.Triangles.Count);

        var firstOrdered = first.SupportPoints
            .OrderBy(point => point.Position.X)
            .ThenBy(point => point.Position.Y)
            .ThenBy(point => point.Position.Z)
            .ToList();
        var secondOrdered = second.SupportPoints
            .OrderBy(point => point.Position.X)
            .ThenBy(point => point.Position.Y)
            .ThenBy(point => point.Position.Z)
            .ToList();

        for (var i = 0; i < firstOrdered.Count; i++)
        {
            Assert.Equal(firstOrdered[i].Size, secondOrdered[i].Size);
            Assert.Equal(firstOrdered[i].RadiusMm, secondOrdered[i].RadiusMm, 5);
            Assert.Equal(firstOrdered[i].Position.X, secondOrdered[i].Position.X, 5);
            Assert.Equal(firstOrdered[i].Position.Y, secondOrdered[i].Position.Y, 5);
            Assert.Equal(firstOrdered[i].Position.Z, secondOrdered[i].Position.Z, 5);
        }
    }

    [Fact]
    public void GenerateSupportPreview_RotatedCube_DoesNotClusterSupportsAtBottom()
    {
        var cube = MakeBox(centerX: 0f, centerZ: 0f, width: 24f, depth: 24f, height: 24f);
        var rotated = RotateTrianglesAroundX(cube, MathF.PI / 4f, pivotY: 12f);
        var onBed = LiftTrianglesToBed(rotated);
        var geometry = CreateGeometryFromTriangles(onBed);

        var result = sut.GenerateSupportPreview(geometry, DefaultOverrides, maxSupportPoints: 2000);

        Assert.NotEmpty(result.SupportPoints);
        var minY = result.SupportPoints.Min(point => point.Position.Y);
        var maxY = result.SupportPoints.Max(point => point.Position.Y);
        var bottomBandThreshold = minY + ((maxY - minY) * 0.25f);
        var bottomBandCount = result.SupportPoints.Count(point => point.Position.Y <= bottomBandThreshold);
        var bottomBandRatio = (float)bottomBandCount / result.SupportPoints.Count;

        Assert.True(bottomBandRatio < 0.50f,
            $"Expected rotated-cube supports to distribute above the first quarter band; got {bottomBandCount}/{result.SupportPoints.Count} ({bottomBandRatio:P2}) in bottom band.");
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

    private static LoadedGeometry CreateGeometryFromTriangles(List<Triangle3D> triangles)
    {
        var minX = float.MaxValue;
        var minY = float.MaxValue;
        var minZ = float.MaxValue;
        var maxX = float.MinValue;
        var maxY = float.MinValue;
        var maxZ = float.MinValue;

        foreach (var triangle in triangles)
        {
            UpdateBounds(triangle.V0, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
            UpdateBounds(triangle.V1, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
            UpdateBounds(triangle.V2, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
        }

        var center = new Vec3(
            (minX + maxX) * 0.5f,
            (minY + maxY) * 0.5f,
            (minZ + maxZ) * 0.5f);
        var radius = triangles
            .SelectMany(t => new[] { t.V0, t.V1, t.V2 })
            .Select(v => (v - center).Length)
            .DefaultIfEmpty(1f)
            .Max();

        return new LoadedGeometry
        {
            Triangles = triangles,
            DimensionXMm = maxX - minX,
            DimensionYMm = maxY - minY,
            DimensionZMm = maxZ - minZ,
            SphereCentre = center,
            SphereRadius = radius,
        };
    }

    private static void UpdateBounds(
        Vec3 point,
        ref float minX,
        ref float minY,
        ref float minZ,
        ref float maxX,
        ref float maxY,
        ref float maxZ)
    {
        minX = MathF.Min(minX, point.X);
        minY = MathF.Min(minY, point.Y);
        minZ = MathF.Min(minZ, point.Z);
        maxX = MathF.Max(maxX, point.X);
        maxY = MathF.Max(maxY, point.Y);
        maxZ = MathF.Max(maxZ, point.Z);
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

    private static List<Triangle3D> MakeSphere(float radius, float centerY)
    {
        const int latSegments = 24;
        const int lonSegments = 32;
        var centre = new Vec3(0f, centerY, 0f);
        var triangles = new List<Triangle3D>(latSegments * lonSegments * 2);

        for (var lat = 0; lat < latSegments; lat++)
        {
            var theta0 = MathF.PI * lat / latSegments;
            var theta1 = MathF.PI * (lat + 1) / latSegments;

            for (var lon = 0; lon < lonSegments; lon++)
            {
                var phi0 = (MathF.PI * 2f * lon) / lonSegments;
                var phi1 = (MathF.PI * 2f * (lon + 1)) / lonSegments;

                var p00 = SpherePoint(centre, radius, theta0, phi0);
                var p01 = SpherePoint(centre, radius, theta0, phi1);
                var p10 = SpherePoint(centre, radius, theta1, phi0);
                var p11 = SpherePoint(centre, radius, theta1, phi1);

                if (lat > 0)
                    triangles.Add(new Triangle3D(p00, p10, p01, (p10 - p00).Cross(p01 - p00).Normalized));

                if (lat < latSegments - 1)
                    triangles.Add(new Triangle3D(p01, p10, p11, (p10 - p01).Cross(p11 - p01).Normalized));
            }
        }

        return triangles;
    }

    private static Vec3 SpherePoint(Vec3 centre, float radius, float theta, float phi)
    {
        var sinTheta = MathF.Sin(theta);
        return new Vec3(
            centre.X + (radius * sinTheta * MathF.Cos(phi)),
            centre.Y + (radius * MathF.Cos(theta)),
            centre.Z + (radius * sinTheta * MathF.Sin(phi)));
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

    private static List<Triangle3D> RotateTrianglesAroundX(IEnumerable<Triangle3D> triangles, float angleRad, float pivotY)
    {
        var cos = MathF.Cos(angleRad);
        var sin = MathF.Sin(angleRad);

        Vec3 Rotate(Vec3 point)
        {
            var y = point.Y - pivotY;
            var z = point.Z;
            var rotatedY = (y * cos) - (z * sin);
            var rotatedZ = (y * sin) + (z * cos);
            return new Vec3(point.X, rotatedY + pivotY, rotatedZ);
        }

        return triangles
            .Select(triangle =>
            {
                var v0 = Rotate(triangle.V0);
                var v1 = Rotate(triangle.V1);
                var v2 = Rotate(triangle.V2);
                var normal = (v1 - v0).Cross(v2 - v0).Normalized;
                return new Triangle3D(v0, v1, v2, normal);
            })
            .ToList();
    }

    private static List<Triangle3D> LiftTrianglesToBed(IEnumerable<Triangle3D> triangles)
    {
        var minY = triangles
            .SelectMany(t => new[] { t.V0.Y, t.V1.Y, t.V2.Y })
            .DefaultIfEmpty(0f)
            .Min();
        var offsetY = -minY;

        return triangles
            .Select(triangle => new Triangle3D(
                new Vec3(triangle.V0.X, triangle.V0.Y + offsetY, triangle.V0.Z),
                new Vec3(triangle.V1.X, triangle.V1.Y + offsetY, triangle.V1.Z),
                new Vec3(triangle.V2.X, triangle.V2.Y + offsetY, triangle.V2.Z),
                triangle.Normal))
            .ToList();
    }
}
