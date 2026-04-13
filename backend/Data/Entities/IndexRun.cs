namespace findamodel.Data.Entities;

public class IndexRun
{
    public Guid Id { get; set; }
    public string? DirectoryFilter { get; set; }
    public string? RelativeModelPath { get; set; }
    public int Flags { get; set; }

    public DateTime RequestedAt { get; set; }
    public DateTime? StartedAt { get; set; }
    public DateTime? CompletedAt { get; set; }
    public double? DurationMs { get; set; }

    public string Status { get; set; } = "running"; // running | success | failed
    public string? Outcome { get; set; } // success | failed
    public string? Error { get; set; }

    public int? TotalFiles { get; set; }
    public int ProcessedFiles { get; set; }

    public List<IndexRunFile> Files { get; set; } = [];
    public List<IndexRunEvent> Events { get; set; } = [];
}
