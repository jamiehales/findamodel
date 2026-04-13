namespace findamodel.Data.Entities;

public class AppConfig
{
    public int Id { get; set; }
    public float DefaultRaftHeightMm { get; set; } = 2f;
    public string Theme { get; set; } = "nord";

    public bool TagGenerationEnabled { get; set; } = false;
    public bool AiDescriptionEnabled { get; set; } = false;
    public string TagGenerationProvider { get; set; } = "internal";
    public string TagGenerationEndpoint { get; set; } = "http://localhost:11434";
    public string TagGenerationModel { get; set; } = "qwen2.5vl:7b";
    public int TagGenerationTimeoutMs { get; set; } = 60000;
    public int TagGenerationMaxTags { get; set; } = 12;
    public float TagGenerationMinConfidence { get; set; } = 0.45f;
    public string TagGenerationPromptTemplate { get; set; } = "";
    public string DescriptionGenerationPromptTemplate { get; set; } = "";

    // Initial setup tracking
    public bool SetupCompleted { get; set; } = false;
    public string? ModelsDirectoryPath { get; set; }

    public DateTime UpdatedAt { get; set; }
}
