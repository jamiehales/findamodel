using System.Globalization;
using System.Security.Cryptography;
using System.Text;

namespace findamodel.Models;

public enum ModelRepairProfile
{
    Safe,
    Standard,
    Aggressive,
}

public sealed record class ModelRepairOptions
{
    public bool Enabled { get; init; } = true;
    public bool StrictMode { get; init; }
    public ModelRepairProfile Profile { get; init; } = ModelRepairProfile.Safe;

    public bool EnableDegenerateRemoval { get; init; } = true;
    public bool EnableDuplicateRemoval { get; init; } = true;
    public bool EnableVertexWeld { get; init; } = true;
    public bool EnableWindingFix { get; init; } = true;
    public bool EnableDustComponentFiltering { get; init; } = true;

    public bool EnableHoleFill { get; init; }
    public bool EnableInternalVoidRepair { get; init; }
    public bool EnableThinSlabDetection { get; init; }

    public bool EnableFallbackRemesh { get; init; }

    public float AreaEpsilonMultiplier { get; init; } = 1f;
    public float EdgeEpsilonMultiplier { get; init; } = 1f;
    public float WeldEpsilonMultiplier { get; init; } = 1f;

    public int DustTriangleThreshold { get; init; } = 8;
    public float DustDiagonalThresholdMm { get; init; } = 0.2f;
    public float MinVoidVolumeMm3 { get; init; } = 1f;
    public float MinWallThicknessMm { get; init; } = 0.05f;

    public int MaxHoleLoopVertices { get; init; } = 256;
    public float MaxHoleDiagonalMm { get; init; } = 5f;
    public int MaxHoleLoopsToCap { get; init; } = 32;

    public int InternalVoidRayCount { get; init; } = 12;
    public float ThinSlabAabbOverlapThreshold { get; init; } = 0.5f;

    public int NonManifoldEdgeFallbackThreshold { get; init; } = 1024;
    public int SelfIntersectionFallbackThreshold { get; init; } = 1024;

    public int MaxFallbackVoxelResolution { get; init; } = 192;

    public string ComputeDeterministicHash()
    {
        var payload = string.Join(",", [
            Enabled.ToString(CultureInfo.InvariantCulture),
            StrictMode.ToString(CultureInfo.InvariantCulture),
            Profile.ToString(),
            EnableDegenerateRemoval.ToString(CultureInfo.InvariantCulture),
            EnableDuplicateRemoval.ToString(CultureInfo.InvariantCulture),
            EnableVertexWeld.ToString(CultureInfo.InvariantCulture),
            EnableWindingFix.ToString(CultureInfo.InvariantCulture),
            EnableDustComponentFiltering.ToString(CultureInfo.InvariantCulture),
            EnableHoleFill.ToString(CultureInfo.InvariantCulture),
            EnableInternalVoidRepair.ToString(CultureInfo.InvariantCulture),
            EnableThinSlabDetection.ToString(CultureInfo.InvariantCulture),
            EnableFallbackRemesh.ToString(CultureInfo.InvariantCulture),
            AreaEpsilonMultiplier.ToString("G9", CultureInfo.InvariantCulture),
            EdgeEpsilonMultiplier.ToString("G9", CultureInfo.InvariantCulture),
            WeldEpsilonMultiplier.ToString("G9", CultureInfo.InvariantCulture),
            DustTriangleThreshold.ToString(CultureInfo.InvariantCulture),
            DustDiagonalThresholdMm.ToString("G9", CultureInfo.InvariantCulture),
            MinVoidVolumeMm3.ToString("G9", CultureInfo.InvariantCulture),
            MinWallThicknessMm.ToString("G9", CultureInfo.InvariantCulture),
            MaxHoleLoopVertices.ToString(CultureInfo.InvariantCulture),
            MaxHoleDiagonalMm.ToString("G9", CultureInfo.InvariantCulture),
            MaxHoleLoopsToCap.ToString(CultureInfo.InvariantCulture),
            InternalVoidRayCount.ToString(CultureInfo.InvariantCulture),
            ThinSlabAabbOverlapThreshold.ToString("G9", CultureInfo.InvariantCulture),
            NonManifoldEdgeFallbackThreshold.ToString(CultureInfo.InvariantCulture),
            SelfIntersectionFallbackThreshold.ToString(CultureInfo.InvariantCulture),
            MaxFallbackVoxelResolution.ToString(CultureInfo.InvariantCulture),
        ]);

        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes(payload));
        return Convert.ToHexString(bytes);
    }

    public static ModelRepairOptions Resolve(IConfiguration config)
    {
        var enabled = config.GetValue<bool?>("Slicing:Repair:Enabled") ?? true;
        var strictMode = config.GetValue<bool?>("Slicing:Repair:StrictMode") ?? false;
        var profileRaw = config["Slicing:Repair:Profile"] ?? "Safe";

        if (!Enum.TryParse<ModelRepairProfile>(profileRaw, ignoreCase: true, out var profile))
            profile = ModelRepairProfile.Safe;

        var options = profile switch
        {
            ModelRepairProfile.Safe => new ModelRepairOptions
            {
                Enabled = enabled,
                StrictMode = strictMode,
                Profile = profile,
                EnableHoleFill = false,
                EnableInternalVoidRepair = false,
                EnableThinSlabDetection = false,
                EnableFallbackRemesh = false,
            },
            ModelRepairProfile.Standard => new ModelRepairOptions
            {
                Enabled = enabled,
                StrictMode = strictMode,
                Profile = profile,
                EnableHoleFill = true,
                EnableInternalVoidRepair = true,
                EnableThinSlabDetection = true,
                EnableFallbackRemesh = false,
            },
            ModelRepairProfile.Aggressive => new ModelRepairOptions
            {
                Enabled = enabled,
                StrictMode = strictMode,
                Profile = profile,
                EnableHoleFill = true,
                EnableInternalVoidRepair = true,
                EnableThinSlabDetection = true,
                EnableFallbackRemesh = true,
            },
            _ => new ModelRepairOptions { Enabled = enabled, StrictMode = strictMode, Profile = ModelRepairProfile.Safe },
        };

        return options with
        {
            AreaEpsilonMultiplier = config.GetValue<float?>("Slicing:Repair:AreaEpsilonMultiplier") ?? options.AreaEpsilonMultiplier,
            EdgeEpsilonMultiplier = config.GetValue<float?>("Slicing:Repair:EdgeEpsilonMultiplier") ?? options.EdgeEpsilonMultiplier,
            WeldEpsilonMultiplier = config.GetValue<float?>("Slicing:Repair:WeldEpsilonMultiplier") ?? options.WeldEpsilonMultiplier,
            DustTriangleThreshold = config.GetValue<int?>("Slicing:Repair:DustTriangleThreshold") ?? options.DustTriangleThreshold,
            DustDiagonalThresholdMm = config.GetValue<float?>("Slicing:Repair:DustDiagonalThresholdMm") ?? options.DustDiagonalThresholdMm,
            MinVoidVolumeMm3 = config.GetValue<float?>("Slicing:Repair:MinVoidVolumeMm3") ?? options.MinVoidVolumeMm3,
            MinWallThicknessMm = config.GetValue<float?>("Slicing:Repair:MinWallThicknessMm") ?? options.MinWallThicknessMm,
            MaxHoleLoopVertices = config.GetValue<int?>("Slicing:Repair:MaxHoleLoopVertices") ?? options.MaxHoleLoopVertices,
            MaxHoleDiagonalMm = config.GetValue<float?>("Slicing:Repair:MaxHoleDiagonalMm") ?? options.MaxHoleDiagonalMm,
            MaxHoleLoopsToCap = config.GetValue<int?>("Slicing:Repair:MaxHoleLoopsToCap") ?? options.MaxHoleLoopsToCap,
            InternalVoidRayCount = config.GetValue<int?>("Slicing:Repair:InternalVoidRayCount") ?? options.InternalVoidRayCount,
            ThinSlabAabbOverlapThreshold = config.GetValue<float?>("Slicing:Repair:ThinSlabAabbOverlapThreshold") ?? options.ThinSlabAabbOverlapThreshold,
            NonManifoldEdgeFallbackThreshold = config.GetValue<int?>("Slicing:Repair:NonManifoldEdgeFallbackThreshold") ?? options.NonManifoldEdgeFallbackThreshold,
            SelfIntersectionFallbackThreshold = config.GetValue<int?>("Slicing:Repair:SelfIntersectionFallbackThreshold") ?? options.SelfIntersectionFallbackThreshold,
            MaxFallbackVoxelResolution = config.GetValue<int?>("Slicing:Repair:MaxFallbackVoxelResolution") ?? options.MaxFallbackVoxelResolution,
        };
    }
}
