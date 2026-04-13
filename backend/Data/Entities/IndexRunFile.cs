namespace findamodel.Data.Entities;

public class IndexRunFile
{
    public Guid Id { get; set; }
    public Guid IndexRunId { get; set; }
    public IndexRun IndexRun { get; set; } = null!;

    public string RelativePath { get; set; } = "";
    public string FileType { get; set; } = "";
    public string Status { get; set; } = "pending"; // pending | processed | skipped | failed
    public bool IsNew { get; set; }
    public bool WasUpdated { get; set; }

    public bool GeneratedPreview { get; set; }
    public bool GeneratedHull { get; set; }
    public bool GeneratedAiTags { get; set; }
    public bool GeneratedAiDescription { get; set; }
    public string? AiGenerationReason { get; set; }

    public string? Message { get; set; }
    public double? DurationMs { get; set; }
    public DateTime? ProcessedAt { get; set; }
}
