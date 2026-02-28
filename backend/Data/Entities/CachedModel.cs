namespace findamodel.Data.Entities;

public class CachedModel
{
    public Guid Id { get; set; }
    public string Checksum { get; set; } = "";   // SHA256 of file content (64 hex chars)
    public string FileName { get; set; } = "";   // filename with extension
    public string Directory { get; set; } = "";  // relative folder path from model root (forward slashes, empty string for root)
    public string FileType { get; set; } = "";   // "stl" or "obj"
    public long FileSize { get; set; }
    public DateTime FileModifiedAt { get; set; }
    public DateTime CachedAt { get; set; }
    public byte[]? PreviewImage { get; set; }    // PNG preview render (null = not yet generated)
    public DateTime? PreviewGeneratedAt { get; set; }

    // FK to the DirectoryConfig record for this model's containing directory
    public Guid? DirectoryConfigId { get; set; }
    public DirectoryConfig? DirectoryConfig { get; set; }

    // Hull coordinates as JSON arrays: [[x1,y1],[x2,y2],...]
    // Projected onto X-Z plane with Y-up coordinate system (bird's eye view)
    public string? ConvexHullCoordinates { get; set; }   // Outer boundary hull (JSON)
    public string? ConcaveHullCoordinates { get; set; }  // Concave/alpha hull (JSON)
    public DateTime? HullGeneratedAt { get; set; }
}
