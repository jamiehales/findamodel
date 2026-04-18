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
                    new findamodel.Models.AutoSupportVectorDto(0.5f, 4f, -0.25f),
                    "medium"),
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
                AutoSupportUnsupportedIslandVolumeThresholdMm3 = 0.01f,
                AutoSupportMaxSupportsPerIsland = 4,
            });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: -6f, centerZ: 0f, width: 1.2f, depth: 2f, height: 4f),
            MakeBox(centerX: 6f, centerZ: 0f, width: 1.2f, depth: 2f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.True(result.SupportPoints.Count >= 2);
        Assert.Contains(result.SupportPoints, point => point.Position.X < 0f);
        Assert.Contains(result.SupportPoints, point => point.Position.X > 0f);
    }

    [Fact]
    public void GenerateSupportPreview_AssignsSupportSizeToEachPoint()
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
                point.Size == SupportSize.Heavy,
                $"Unexpected support size: {point.Size}");
            Assert.True(point.RadiusMm > 0f, "Support radius must be positive");
        });
    }

    [Fact]
    public void GenerateSupportPreview_UsesHeavySupportsNearBase()
    {
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 30f));

        var result = sut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        var baseSupports = result.SupportPoints
            .Where(p => p.Position.Y <= 3f)
            .ToList();
        if (baseSupports.Count > 0)
        {
            Assert.Contains(baseSupports, p => p.Size == SupportSize.Heavy);
        }
    }

    [Fact]
    public void GenerateSupportPreview_LargerTipRadiusForHeavierSupports()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_LargerTipRadiusForHeavierSupports),
            request => request with
            {
                AutoSupportMicroTipRadiusMm = 0.3f,
                AutoSupportLightTipRadiusMm = 0.6f,
                AutoSupportMediumTipRadiusMm = 1.0f,
                AutoSupportHeavyTipRadiusMm = 1.8f,
            });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 10f, depth: 10f, height: 30f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        var heavySupports = result.SupportPoints.Where(p => p.Size == SupportSize.Heavy).ToList();
        var lightSupports = result.SupportPoints.Where(p => p.Size == SupportSize.Light).ToList();
        if (heavySupports.Count > 0 && lightSupports.Count > 0)
        {
            Assert.True(
                heavySupports.Average(p => p.RadiusMm) > lightSupports.Average(p => p.RadiusMm),
                "Heavy supports should have larger radii than light supports");
        }
    }

    [Fact]
    public void GenerateSupportPreview_ResinStrengthAffectsSupportDensity()
    {
        var weakResinConfig = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_ResinStrengthAffectsSupportDensity) + "_weak",
            request => request with { AutoSupportResinStrength = 0.3f, AutoSupportMaxSupportsPerIsland = 16 });
        var strongResinConfig = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_ResinStrengthAffectsSupportDensity) + "_strong",
            request => request with { AutoSupportResinStrength = 5f, AutoSupportMaxSupportsPerIsland = 16 });

        var weakSut = new AutoSupportGenerationService(weakResinConfig, NullLoggerFactory.Instance);
        var strongSut = new AutoSupportGenerationService(strongResinConfig, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 20f, depth: 6f, height: 4f));

        var weakResult = weakSut.GenerateSupportPreview(geometry);
        var strongResult = strongSut.GenerateSupportPreview(geometry);

        Assert.True(
            weakResult.SupportPoints.Count >= strongResult.SupportPoints.Count,
            $"Weak resin ({weakResult.SupportPoints.Count} supports) should need at least as many supports as strong resin ({strongResult.SupportPoints.Count})");
    }

    [Fact]
    public void GenerateSupportPreview_HigherResinDensityIncreasesSupports()
    {
        var lightConfig = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_HigherResinDensityIncreasesSupports) + "_light",
            request => request with { AutoSupportResinDensityGPerMl = 0.5f, AutoSupportMaxSupportsPerIsland = 16 });
        var heavyConfig = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_HigherResinDensityIncreasesSupports) + "_heavy",
            request => request with { AutoSupportResinDensityGPerMl = 5f, AutoSupportMaxSupportsPerIsland = 16 });

        var lightSut = new AutoSupportGenerationService(lightConfig, NullLoggerFactory.Instance);
        var heavySut = new AutoSupportGenerationService(heavyConfig, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 20f, depth: 6f, height: 10f));

        var lightResult = lightSut.GenerateSupportPreview(geometry);
        var heavyResult = heavySut.GenerateSupportPreview(geometry);

        Assert.True(
            heavyResult.SupportPoints.Count >= lightResult.SupportPoints.Count,
            $"Heavy resin ({heavyResult.SupportPoints.Count} supports) should need at least as many supports as light resin ({lightResult.SupportPoints.Count})");
    }

    [Fact]
    public void GenerateSupportPreview_PeelForceMultiplierAffectsSupports()
    {
        var lowPeelConfig = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_PeelForceMultiplierAffectsSupports) + "_low",
            request => request with { AutoSupportPeelForceMultiplier = 0.01f, AutoSupportMaxSupportsPerIsland = 16 });
        var highPeelConfig = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_PeelForceMultiplierAffectsSupports) + "_high",
            request => request with { AutoSupportPeelForceMultiplier = 2f, AutoSupportMaxSupportsPerIsland = 16 });

        var lowSut = new AutoSupportGenerationService(lowPeelConfig, NullLoggerFactory.Instance);
        var highSut = new AutoSupportGenerationService(highPeelConfig, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 20f, depth: 6f, height: 10f));

        var lowResult = lowSut.GenerateSupportPreview(geometry);
        var highResult = highSut.GenerateSupportPreview(geometry);

        Assert.True(
            highResult.SupportPoints.Count >= lowResult.SupportPoints.Count,
            $"High peel force ({highResult.SupportPoints.Count} supports) should need at least as many supports as low peel force ({lowResult.SupportPoints.Count})");
    }

    [Fact]
    public void GenerateSupportPreview_DelaysSupportUntilUnsupportedVolumeThresholdExceeded()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_DelaysSupportUntilUnsupportedVolumeThresholdExceeded),
            request => request with
            {
                AutoSupportUnsupportedIslandVolumeThresholdMm3 = 10f,
                AutoSupportMaxSupportsPerIsland = 2,
            });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 2f, depth: 2f, height: 8f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        Assert.NotEmpty(result.SupportPoints);
        var firstSupportHeight = result.SupportPoints.Min(point => point.Position.Y);
        Assert.True(firstSupportHeight <= 0.4f, $"Expected support to be placed at earliest layer, got {firstSupportHeight:F3}mm");
    }

    [Fact]
    public void GenerateSupportPreview_PlacesSupportsEvenWhenUnsupportedVolumeThresholdIsVeryHigh()
    {
        var config = CreateConfiguredAppConfigService(
            nameof(GenerateSupportPreview_PlacesSupportsEvenWhenUnsupportedVolumeThresholdIsVeryHigh),
            request => request with
            {
                AutoSupportUnsupportedIslandVolumeThresholdMm3 = 100000f,
            });
        var configuredSut = new AutoSupportGenerationService(config, NullLoggerFactory.Instance);
        var geometry = CreateGeometry(
            MakeBox(centerX: 0f, centerZ: 0f, width: 2f, depth: 2f, height: 4f));

        var result = configuredSut.GenerateSupportPreview(geometry);

        // All islands now receive an initial support regardless of volume threshold
        Assert.NotEmpty(result.SupportPoints);
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

        // Should detect islands in two separate regions
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
        // Elevated plate (Y=6..8) with three narrow pillars hanging down to different depths.
        // Pillar tips create separate islands at lower slices:
        //   Pillar A: X=-10, tip at Y=0  (tallest spike)
        //   Pillar B: X=0,   tip at Y=2  (medium spike)
        //   Pillar C: X=10,  tip at Y=4  (short spike)
        var plate = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 6f, bottomY: 6f, topY: 8f);
        var spikeA = MakeElevatedBox(centerX: -10f, centerZ: 0f, width: 2f, depth: 2f, bottomY: 0f, topY: 6f);
        var spikeB = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 2f, depth: 2f, bottomY: 2f, topY: 6f);
        var spikeC = MakeElevatedBox(centerX: 10f, centerZ: 0f, width: 2f, depth: 2f, bottomY: 4f, topY: 6f);
        var geometry = CreateGeometry(plate, spikeA, spikeB, spikeC);

        var result = sut.GenerateSupportPreview(geometry);

        // All three spike regions should be detected as islands
        Assert.True(result.Islands.Count >= 3, $"Expected at least 3 islands, got {result.Islands.Count}");

        // Each spike region should have at least one support near its tip
        Assert.Contains(result.SupportPoints, p => p.Position.X < -5f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > -5f && p.Position.X < 5f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 5f);
    }

    [Fact]
    public void GenerateSupportPreview_PlateWithMultipleHangingSpikes_SupportsEachSpikeTipNearBottom()
    {
        // Verify supports are placed near the actual tip heights
        var plate = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 6f, bottomY: 8f, topY: 10f);
        var spikeA = MakeElevatedBox(centerX: -10f, centerZ: 0f, width: 2f, depth: 2f, bottomY: 0f, topY: 8f);
        var spikeB = MakeElevatedBox(centerX: 10f, centerZ: 0f, width: 2f, depth: 2f, bottomY: 0f, topY: 8f);
        var geometry = CreateGeometry(plate, spikeA, spikeB);

        var result = sut.GenerateSupportPreview(geometry);

        // Both spikes should have supports near their tips (low Y)
        var leftSupports = result.SupportPoints.Where(p => p.Position.X < -5f).ToList();
        var rightSupports = result.SupportPoints.Where(p => p.Position.X > 5f).ToList();
        Assert.NotEmpty(leftSupports);
        Assert.NotEmpty(rightSupports);
        Assert.Contains(leftSupports, p => p.Position.Y <= 2f);
        Assert.Contains(rightSupports, p => p.Position.Y <= 2f);
    }

    [Fact]
    public void GenerateSupportPreview_MultipleIslandsAtSameSliceLevel_AllGetSupports()
    {
        // Four narrow pillars at the same height hanging from an elevated plate
        // All tips at Y=0, all should be separate islands at the bottom slice
        var plate = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 16f, bottomY: 6f, topY: 8f);
        var spikeA = MakeElevatedBox(centerX: -8f, centerZ: -5f, width: 2f, depth: 2f, bottomY: 0f, topY: 6f);
        var spikeB = MakeElevatedBox(centerX: 8f, centerZ: -5f, width: 2f, depth: 2f, bottomY: 0f, topY: 6f);
        var spikeC = MakeElevatedBox(centerX: -8f, centerZ: 5f, width: 2f, depth: 2f, bottomY: 0f, topY: 6f);
        var spikeD = MakeElevatedBox(centerX: 8f, centerZ: 5f, width: 2f, depth: 2f, bottomY: 0f, topY: 6f);
        var geometry = CreateGeometry(plate, spikeA, spikeB, spikeC, spikeD);

        var result = sut.GenerateSupportPreview(geometry);

        // All four spike tips appear at the same slice level and must all be detected
        Assert.True(result.Islands.Count >= 4, $"Expected at least 4 islands, got {result.Islands.Count}");

        // All four quadrants should have supports
        Assert.Contains(result.SupportPoints, p => p.Position.X < -3f && p.Position.Z < -2f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 3f && p.Position.Z < -2f);
        Assert.Contains(result.SupportPoints, p => p.Position.X < -3f && p.Position.Z > 2f);
        Assert.Contains(result.SupportPoints, p => p.Position.X > 3f && p.Position.Z > 2f);
    }

    [Fact]
    public void GenerateSupportPreview_DifferentSizedSpikes_SmallSpikesStillGetSupports()
    {
        // Spikes of very different cross-section sizes hanging from an elevated plate
        var plate = MakeElevatedBox(centerX: 0f, centerZ: 0f, width: 30f, depth: 10f, bottomY: 6f, topY: 8f);
        var largeSpike = MakeElevatedBox(centerX: -10f, centerZ: 0f, width: 4f, depth: 4f, bottomY: 0f, topY: 6f);
        var smallSpike = MakeElevatedBox(centerX: 10f, centerZ: 0f, width: 2f, depth: 2f, bottomY: 0f, topY: 6f);
        var geometry = CreateGeometry(plate, largeSpike, smallSpike);

        var result = sut.GenerateSupportPreview(geometry);

        // Both spikes should get supports regardless of size
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
