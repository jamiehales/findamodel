using findamodel.Models;
using findamodel.Services;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public sealed class AutoSupportSettingsPreviewServiceTests
{
    [Fact]
    public async Task GeneratePreviewAsync_ReturnsBuiltInScenariosAndCachedGeometry()
    {
        var cachePath = Path.Combine(Path.GetTempPath(), $"autosupport-preview-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(cachePath);

        try
        {
            var config = new ConfigurationBuilder()
                .AddInMemoryCollection(new Dictionary<string, string?>
                {
                    ["Cache:AutoSupportsPath"] = cachePath,
                })
                .Build();

            var loggerFactory = NullLoggerFactory.Instance;
            var sut = new AutoSupportSettingsPreviewService(
                new AutoSupportGenerationV3Service(loggerFactory),
                new ModelLoaderService(loggerFactory),
                new MeshTransferService(),
                config,
                loggerFactory);

            var request = new AutoSupportSettingsPreviewRequest(
                new AutoSupportSettingsPreviewTuningRequest(
                    BedMarginMm: 2f,
                    MinVoxelSizeMm: 0.8f,
                    MaxVoxelSizeMm: 2f,
                    MinLayerHeightMm: 0.75f,
                    MaxLayerHeightMm: 1.5f,
                    MergeDistanceMm: 2.5f,
                    MinIslandAreaMm2: 4f,
                    ResinStrength: 1f,
                    CrushForceThreshold: 20f,
                    MaxAngularForce: 40f,
                    PeelForceMultiplier: 0.15f,
                    LightTipRadiusMm: 0.7f,
                    MediumTipRadiusMm: 1f,
                    HeavyTipRadiusMm: 1.5f));

            var result = await sut.GeneratePreviewAsync(request);

            Assert.Equal(6, result.Scenarios.Count);
            Assert.All(result.Scenarios, scenario => Assert.Equal("completed", scenario.Status));

            foreach (var scenario in result.Scenarios)
            {
                var envelope = sut.GetScenarioEnvelope(result.PreviewId, scenario.ScenarioId);
                Assert.NotNull(envelope);
                Assert.NotEmpty(envelope!);
            }
        }
        finally
        {
            try
            {
                if (Directory.Exists(cachePath))
                    Directory.Delete(cachePath, recursive: true);
            }
            catch
            {
            }
        }
    }

    [Fact]
    public async Task GeneratePreviewAsync_WithScenarioId_GeneratesOnlyRequestedScenario()
    {
        var cachePath = Path.Combine(Path.GetTempPath(), $"autosupport-preview-tests-{Guid.NewGuid():N}");
        Directory.CreateDirectory(cachePath);

        try
        {
            var config = new ConfigurationBuilder()
                .AddInMemoryCollection(new Dictionary<string, string?>
                {
                    ["Cache:AutoSupportsPath"] = cachePath,
                })
                .Build();

            var loggerFactory = NullLoggerFactory.Instance;
            var sut = new AutoSupportSettingsPreviewService(
                new AutoSupportGenerationV3Service(loggerFactory),
                new ModelLoaderService(loggerFactory),
                new MeshTransferService(),
                config,
                loggerFactory);

            var request = new AutoSupportSettingsPreviewRequest(
                new AutoSupportSettingsPreviewTuningRequest(
                    BedMarginMm: 2f,
                    MinVoxelSizeMm: 0.8f,
                    MaxVoxelSizeMm: 2f,
                    MinLayerHeightMm: 0.75f,
                    MaxLayerHeightMm: 1.5f,
                    MergeDistanceMm: 2.5f,
                    MinIslandAreaMm2: 4f,
                    ResinStrength: 1f,
                    CrushForceThreshold: 20f,
                    MaxAngularForce: 40f,
                    PeelForceMultiplier: 0.15f,
                    LightTipRadiusMm: 0.7f,
                    MediumTipRadiusMm: 1f,
                    HeavyTipRadiusMm: 1.5f),
                ScenarioId: "cube-40");

            var result = await sut.GeneratePreviewAsync(request);

            Assert.Equal(6, result.Scenarios.Count);
            var generated = Assert.Single(result.Scenarios, s => s.Status == "completed");
            Assert.Equal("cube-40", generated.ScenarioId);

            var deferred = result.Scenarios.Where(s => s.Status == "not-generated").ToList();
            Assert.Equal(5, deferred.Count);
            Assert.All(deferred, scenario =>
            {
                Assert.Equal(0, scenario.SupportCount);
                Assert.Null(scenario.SupportPoints);
                Assert.Null(scenario.Islands);
            });

            foreach (var scenario in result.Scenarios)
            {
                var envelope = sut.GetScenarioEnvelope(result.PreviewId, scenario.ScenarioId);
                if (scenario.ScenarioId == "cube-40")
                {
                    Assert.NotNull(envelope);
                    Assert.NotEmpty(envelope!);
                }
                else
                {
                    Assert.Null(envelope);
                }
            }
        }
        finally
        {
            try
            {
                if (Directory.Exists(cachePath))
                    Directory.Delete(cachePath, recursive: true);
            }
            catch
            {
            }
        }
    }
}
