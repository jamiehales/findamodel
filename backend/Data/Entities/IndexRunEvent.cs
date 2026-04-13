namespace findamodel.Data.Entities;

public class IndexRunEvent
{
    public Guid Id { get; set; }
    public Guid IndexRunId { get; set; }
    public IndexRun IndexRun { get; set; } = null!;

    public DateTime CreatedAt { get; set; }
    public string Level { get; set; } = "info";
    public string Message { get; set; } = "";
    public string? RelativePath { get; set; }
}
