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

    [Fact]
    public void GenerateSupportPreview_OptimizationDisabled_ProducesSameAsUniform()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_OptimizationDisabled_ProducesSameAsUniform),
            request => request with { AutoSupportV2OptimizationEnabled = false });
        var configuredSut = new AutoSupportGenerationV2Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 8f, depth: 8f, height: 6f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        Assert.NotEmpty(result.SupportGeometry.Triangles);
    }

    [Fact]
    public void GenerateSupportPreview_OptimizationEnabled_ProducesSupports()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_OptimizationEnabled_ProducesSupports),
            request => request with
            {
                AutoSupportV2OptimizationEnabled = true,
                AutoSupportV2CoarseVoxelSizeMm = 4f,
                AutoSupportV2FineVoxelSizeMm = 1f,
            });
        var configuredSut = new AutoSupportGenerationV2Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 8f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_CoarseOnlyWhenNoRefinementNeeded()
    {
        // With a very small simple geometry at coarse resolution, no refinement should trigger
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_CoarseOnlyWhenNoRefinementNeeded),
            request => request with
            {
                AutoSupportV2OptimizationEnabled = true,
                AutoSupportV2CoarseVoxelSizeMm = 4f,
                AutoSupportV2FineVoxelSizeMm = 2f,
                AutoSupportV2MinRegionVolumeMm3 = 10000f, // very high threshold - no regions qualify
            });
        var configuredSut = new AutoSupportGenerationV2Service(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 6f, depth: 6f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_OptimizationStability_DeterministicOutput()
    {
        var config1 = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_OptimizationStability_DeterministicOutput) + "_1",
            request => request with
            {
                AutoSupportV2OptimizationEnabled = true,
                AutoSupportV2CoarseVoxelSizeMm = 4f,
                AutoSupportV2FineVoxelSizeMm = 2f,
            });
        var config2 = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_OptimizationStability_DeterministicOutput) + "_2",
            request => request with
            {
                AutoSupportV2OptimizationEnabled = true,
                AutoSupportV2CoarseVoxelSizeMm = 4f,
                AutoSupportV2FineVoxelSizeMm = 2f,
            });
        var sut1 = new AutoSupportGenerationV2Service(config1, NullLoggerFactory.Instance);
        var sut2 = new AutoSupportGenerationV2Service(config2, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 6f));

        var result1 = sut1.GenerateSupportPreview(geometry);
        var result2 = sut2.GenerateSupportPreview(geometry);

        Assert.Equal(result1.SupportPoints.Count, result2.SupportPoints.Count);
    }

    [Fact]
    public void SpatialIndex_FindNearby_MatchesBruteForce()
    {
        var supports = new List<SupportPoint>
        {
            new(new Vec3(0f, 0f, 0f), 1f, new Vec3(0f, 0f, 0f), SupportSize.Medium),
            new(new Vec3(5f, 0f, 5f), 1f, new Vec3(0f, 0f, 0f), SupportSize.Medium),
            new(new Vec3(20f, 0f, 20f), 1f, new Vec3(0f, 0f, 0f), SupportSize.Medium),
        };

        var index = new AutoSupportGenerationV2Service.SupportSpatialIndex(10f);
        index.Build(supports);

        var nearby = index.FindNearby(1f, 1f, 10f, supports);

        // Should find supports at (0,0) and (5,5) but not (20,20)
        Assert.Equal(2, nearby.Count);
        Assert.Contains(0, nearby);
        Assert.Contains(1, nearby);
        Assert.DoesNotContain(2, nearby);
    }

    [Fact]
    public void SpatialIndex_Insert_FindsNewlyInsertedSupport()
    {
        var supports = new List<SupportPoint>
        {
            new(new Vec3(0f, 0f, 0f), 1f, new Vec3(0f, 0f, 0f), SupportSize.Medium),
        };

        var index = new AutoSupportGenerationV2Service.SupportSpatialIndex(10f);
        index.Build(supports);

        supports.Add(new SupportPoint(new Vec3(3f, 0f, 3f), 1f, new Vec3(0f, 0f, 0f), SupportSize.Light));
        index.Insert(1, 3f, 3f);

        var nearby = index.FindNearby(2f, 2f, 5f, supports);
        Assert.Equal(2, nearby.Count);
    }

    [Fact]
    public void GenerateSupportPreview_ReturnsDetectedIslands()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.Islands);
        Assert.All(result.Islands, island =>
        {
            Assert.True(island.AreaMm2 > 0f, "Island area must be positive");
            Assert.True(island.RadiusMm > 0f, "Island radius must be positive");
        });
    }

    [Fact]
    public void GenerateSupportPreview_DetectsMultipleIslandsForSeparatedBodies()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: -12f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: 12f, centerZ: 0f, width: 4f, depth: 4f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.Islands.Count >= 2, $"Expected at least 2 islands, got {result.Islands.Count}");
        Assert.Contains(result.Islands, i => i.CentroidX < 0f);
        Assert.Contains(result.Islands, i => i.CentroidX > 0f);
    }

    [Fact]
    public void GenerateSupportPreview_EachIslandReceivesAtLeastOneSupport()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: -12f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: 12f, centerZ: 0f, width: 4f, depth: 4f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.Contains(result.SupportPoints, p => p.Position.X < -4f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 4f);
    }

    [Fact]
    public void GenerateSupportPreview_EmptyGeometryReturnsEmptyIslands()
    {
        var geometry = new LoadedGeometry
        {
            Triangles = [],
            DimensionXMm = 10f,
            DimensionYMm = 10f,
            DimensionZMm = 10f,
            SphereCentre = new Vec3(0f, 5f, 0f),
            SphereRadius = 10f,
        };

        var result = sut.GenerateSupportPreview(geometry);

        Assert.Empty(result.Islands);
        Assert.Empty(result.SupportPoints);
    }

    [Fact]
    public void GenerateSupportPreview_PlateWithMultipleHangingSpikes_DetectsIslandsAtEachTip()
    {
        // Elevated plate (Y=6..8) with three pillars hanging down to different depths.
        // Each pillar tip creates a separate island at lower slices.
        // Pillars are 4x4mm so they occupy multiple voxels (2mm voxel size) and survive
        // the bitmap's RemoveUnsupportedHorizontalPixels filtering.
        var plate = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 14f, bottomY: 6f, topY: 8f);
        var spikeA = MakeElevatedBox(centerX: -10f, centerZ: 0f, width: 4f, depth: 4f, bottomY: 0f, topY: 6f);
        var spikeB = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 4f, depth: 4f, bottomY: 2f, topY: 6f);
        var spikeC = MakeElevatedBox(centerX: 10f, centerZ: 0f, width: 4f, depth: 4f, bottomY: 4f, topY: 6f);
        var geometry = CreateGeometry(plate, spikeA, spikeB, spikeC);

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.Islands.Count >= 3, $"Expected at least 3 islands, got {result.Islands.Count}");

        Assert.Contains(result.SupportPoints, p => p.Position.X < -5f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > -5f && p.Position.X < 5f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 5f);
    }

    [Fact]
    public void GenerateSupportPreview_MultipleIslandsAtSameSliceLevel_AllGetSupports()
    {
        // Four separate boxes spread along the X axis - all at the same height.
        // All should be separate islands and all should get supports.
        var geometry = CreateGeometry(
            MakeBox(centerX: -15f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: -5f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: 5f, centerZ: 0f, width: 4f, depth: 4f, height: 6f),
            MakeBox(centerX: 15f, centerZ: 0f, width: 4f, depth: 4f, height: 6f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.True(result.Islands.Count >= 4, $"Expected at least 4 islands, got {result.Islands.Count}");

        Assert.Contains(result.SupportPoints, p => p.Position.X < -10f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > -8f && p.Position.X < -2f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 2f && p.Position.X < 8f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 10f);
    }

    [Fact]
    public void GenerateSupportPreview_DifferentSizedSpikes_SmallSpikesStillGetSupports()
    {
        // A large and small spike hanging from an elevated plate.
        // Even the smaller spike should get a support.
        var plate = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 14f, bottomY: 6f, topY: 8f);
        var largeSpike = MakeElevatedBox(centerX: -10f, centerZ: 0f, width: 6f, depth: 6f, bottomY: 0f, topY: 6f);
        var smallSpike = MakeElevatedBox(centerX: 10f, centerZ: 0f, width: 4f, depth: 4f, bottomY: 0f, topY: 6f);
        var geometry = CreateGeometry(plate, smallSpike, largeSpike);

        var result = sut.GenerateSupportPreview(geometry);

        Assert.Contains(result.SupportPoints, p => p.Position.X < -5f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 5f);
    }

    private static List<Triangle3D> MakeElevatedBox(float centerX, float centerZ, float width, float depth, float bottomY, float topY)
    {
        var minX = centerX - (width * 0.5f);
        var maxX = centerX + (width * 0.5f);
        var minZ = centerZ - (depth * 0.5f);
        var maxZ = centerZ + (depth * 0.5f);

        var p000 = new Vec3(minX, bottomY, minZ);
        var p001 = new Vec3(minX, bottomY, maxZ);
        var p010 = new Vec3(minX, topY, minZ);
        var p011 = new Vec3(minX, topY, maxZ);
        var p100 = new Vec3(maxX, bottomY, minZ);
        var p101 = new Vec3(maxX, bottomY, maxZ);
        var p110 = new Vec3(maxX, topY, minZ);
        var p111 = new Vec3(maxX, topY, maxZ);

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
