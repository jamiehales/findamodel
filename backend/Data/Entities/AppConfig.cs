namespace findamodel.Data.Entities;

public class AppConfig
{
    public int Id { get; set; }
    public float DefaultRaftHeightMm { get; set; } = 2f;
    public string Theme { get; set; } = "nord";
    public bool GeneratePreviewsEnabled { get; set; } = true;
    public int MinimumPreviewGenerationVersion { get; set; }

    public bool TagGenerationEnabled { get; set; } = false;
    public bool AiDescriptionEnabled { get; set; } = false;
    public string TagGenerationProvider { get; set; } = "internal";
    public string TagGenerationEndpoint { get; set; } = "http://localhost:11434";
    public string? TagGenerationModel { get; set; }
    public int TagGenerationTimeoutMs { get; set; } = 60000;
    public int TagGenerationMaxTags { get; set; } = 12;
    public float TagGenerationMinConfidence { get; set; } = 0.45f;
    public string TagGenerationPromptTemplate { get; set; } = "";
    public string DescriptionGenerationPromptTemplate { get; set; } = "";

    public float AutoSupportBedMarginMm { get; set; } = 2f;
    public float AutoSupportMinVoxelSizeMm { get; set; } = 0.8f;
    public float AutoSupportMaxVoxelSizeMm { get; set; } = 2f;
    public float AutoSupportMinLayerHeightMm { get; set; } = 0.75f;
    public float AutoSupportMaxLayerHeightMm { get; set; } = 1.5f;
    public float AutoSupportMergeDistanceMm { get; set; } = 2.5f;
    public float AutoSupportMinIslandAreaMm2 { get; set; } = 4f;
    public float AutoSupportMaxSupportDistanceMm { get; set; } = 10f;
    public float AutoSupportUnsupportedIslandVolumeThresholdMm3 { get; set; } = 1f;
    public float AutoSupportPullForceThreshold { get; set; } = 3f;
    public float AutoSupportSphereRadiusMm { get; set; } = 1.2f;
    public int AutoSupportMaxSupportsPerIsland { get; set; } = 6;

    public float AutoSupportResinStrength { get; set; } = 1f;
    public float AutoSupportResinDensityGPerMl { get; set; } = 1.25f;
    public float AutoSupportPeelForceMultiplier { get; set; } = 0.15f;
    public float AutoSupportMicroTipRadiusMm { get; set; } = 0.4f;
    public float AutoSupportLightTipRadiusMm { get; set; } = 0.7f;
    public float AutoSupportMediumTipRadiusMm { get; set; } = 1f;
    public float AutoSupportHeavyTipRadiusMm { get; set; } = 1.5f;
    public float AutoSupportV2VoxelSizeMm { get; set; } = 2f;

    public bool AutoSupportV2OptimizationEnabled { get; set; } = true;
    public float AutoSupportV2CoarseVoxelSizeMm { get; set; } = 4f;
    public float AutoSupportV2FineVoxelSizeMm { get; set; } = 0.5f;
    public float AutoSupportV2RefinementMarginMm { get; set; } = 2.0f;
    public int AutoSupportV2RefinementMaxRegions { get; set; } = 12;
    public float AutoSupportV2RiskForceMarginRatio { get; set; } = 0.2f;
    public float AutoSupportV2MinRegionVolumeMm3 { get; set; } = 8.0f;

    // Initial setup tracking
    public bool SetupCompleted { get; set; } = false;
    public string? ModelsDirectoryPath { get; set; }

    public DateTime UpdatedAt { get; set; }
}
