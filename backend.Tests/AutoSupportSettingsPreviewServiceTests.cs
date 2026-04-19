using findamodel.Models;
using findamodel.Services;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using System.Buffers.Binary;
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

            Assert.Equal(7, result.Scenarios.Count);
            Assert.All(result.Scenarios, scenario => Assert.Equal("completed", scenario.Status));
            Assert.Contains(result.Scenarios, scenario => scenario.ScenarioId == "donut-40");

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

            Assert.Equal(7, result.Scenarios.Count);
            var generated = Assert.Single(result.Scenarios, s => s.Status == "completed");
            Assert.Equal("cube-40", generated.ScenarioId);

            var deferred = result.Scenarios.Where(s => s.Status == "not-generated").ToList();
            Assert.Equal(6, deferred.Count);
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

    [Fact]
    public async Task GeneratePreviewAsync_DonutAndPlane_EncodeNonFlatYDimension()
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

            var tuning = new AutoSupportSettingsPreviewTuningRequest(
                BedMarginMm: 2f,
                MinVoxelSizeMm: 1.5f,
                MaxVoxelSizeMm: 2f,
                MinLayerHeightMm: 2f,
                MaxLayerHeightMm: 2f,
                MergeDistanceMm: 2.5f,
                MinIslandAreaMm2: 4f,
                ResinStrength: 1f,
                CrushForceThreshold: 20f,
                MaxAngularForce: 40f,
                PeelForceMultiplier: 0.15f,
                LightTipRadiusMm: 0.7f,
                MediumTipRadiusMm: 1f,
                HeavyTipRadiusMm: 1.5f,
                SuctionMultiplier: 1f,
                AreaGrowthThreshold: 1f,
                AreaGrowthMultiplier: 1f,
                GravityEnabled: false,
                DragCoefficientMultiplier: 0f,
                MinFeatureWidthMm: 1f,
                ShrinkagePercent: 0f,
                ShrinkageEdgeBias: 0f);

            var donutEnvelope = await GenerateSingleScenarioEnvelopeAsync(sut, tuning, "donut-40");
            var parallelEnvelope = await GenerateSingleScenarioEnvelopeAsync(sut, tuning, "thin-plane-parallel");
            var angledEnvelope = await GenerateSingleScenarioEnvelopeAsync(sut, tuning, "thin-plane-30deg");
            Assert.NotNull(donutEnvelope);
            Assert.NotNull(parallelEnvelope);
            Assert.NotNull(angledEnvelope);

            var donutBodyDimY = ReadBodyDimYFromEnvelope(donutEnvelope!);
            var parallelBodyDimY = ReadBodyDimYFromEnvelope(parallelEnvelope!);
            var angledBodyDimY = ReadBodyDimYFromEnvelope(angledEnvelope!);

            Assert.True(donutBodyDimY > 5f, $"Expected 3D donut Y dimension, got {donutBodyDimY:F3} mm");
            Assert.InRange(parallelBodyDimY, 1.8f, 2.2f);
            Assert.True(angledBodyDimY > 2f, $"Expected angled box to have >2mm Y extent, got {angledBodyDimY:F3} mm");
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

    private static async Task<byte[]?> GenerateSingleScenarioEnvelopeAsync(
        AutoSupportSettingsPreviewService sut,
        AutoSupportSettingsPreviewTuningRequest tuning,
        string scenarioId)
    {
        var result = await sut.GeneratePreviewAsync(new AutoSupportSettingsPreviewRequest(
            tuning,
            ScenarioId: scenarioId));

        var completed = Assert.Single(result.Scenarios, s => s.Status == "completed");
        Assert.Equal(scenarioId, completed.ScenarioId);
        return sut.GetScenarioEnvelope(result.PreviewId, scenarioId);
    }

    private static float ReadBodyDimYFromEnvelope(byte[] envelope)
    {
        var bodyLength = BinaryPrimitives.ReadInt32LittleEndian(envelope.AsSpan(0, 4));
        Assert.True(bodyLength > 56);

        var body = envelope.AsSpan(4, bodyLength);
        // MeshTransferService header layout: dimY at bytes 20..24
        return BinaryPrimitives.ReadSingleLittleEndian(body.Slice(20, 4));
    }
}
