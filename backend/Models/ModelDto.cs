namespace findamodel.Models;

public class ModelDto
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public string RelativePath { get; set; } = "";
    public string FileType { get; set; } = "";
    public long FileSize { get; set; }
    public string FileUrl { get; set; } = "";
    public bool HasPreview { get; set; }
    public string? PreviewUrl { get; set; }

    // Resolved metadata from DirectoryConfig ancestry
    public string? Author { get; set; }
    public string? Collection { get; set; }
    public string? Subcollection { get; set; }
    public string? Category { get; set; }   // "Bust" | "Miniature" | "Uncategorized" | null
    public string? Type { get; set; }        // "Whole" | "Part" | null
    public bool? Supported { get; set; }

    // Hull coordinates as JSON: [[x,z],[x,z],...] projected onto X-Z plane (bird's eye with Y-up)
    public string? ConvexHull { get; set; }
    public string? ConcaveHull { get; set; }
}
