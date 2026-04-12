namespace findamodel.Data.Entities;

public class AppConfig
{
    public int Id { get; set; }
    public float DefaultRaftHeightMm { get; set; } = 2f;
    public string Theme { get; set; } = "nord";

    public bool TagGenerationEnabled { get; set; } = true;
    public string TagGenerationProvider { get; set; } = "internal";
    public string TagGenerationEndpoint { get; set; } = "http://localhost:11434";
    public string TagGenerationModel { get; set; } = "qwen2.5vl:7b";
    public int TagGenerationTimeoutMs { get; set; } = 60000;
    public bool TagGenerationAutoApply { get; set; } = true;
    public int TagGenerationMaxTags { get; set; } = 12;
    public float TagGenerationMinConfidence { get; set; } = 0.45f;

    public DateTime UpdatedAt { get; set; }
}
