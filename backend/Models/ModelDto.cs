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
    public string? Creator { get; set; }
    public string? Collection { get; set; }
    public string? Subcollection { get; set; }
    public string? Category { get; set; }   // "Bust" | "Miniature" | "Uncategorized" | null
    public string? Type { get; set; }        // "Whole" | "Part" | null
    public string? Material { get; set; }    // "FDM" | "Resin" | "Any" | null
    public bool? Supported { get; set; }

    // Hull coordinates as JSON: [[x,z],[x,z],...] projected onto X-Z plane (bird's eye with Y-up)
    public string? ConvexHull { get; set; }
    public string? ConcaveHull { get; set; }
    public string? ConvexSansRaftHull { get; set; }  // Convex hull excluding vertices below RaftOffsetMm
    public float RaftOffsetMm { get; set; }          // Y cutoff used for sans-raft hull calculation

    // Geometry metadata (mm, Y-up centred coordinate system: X/Z at origin, base at Y=0)
    public float? DimensionXMm { get; set; }
    public float? DimensionYMm { get; set; }
    public float? DimensionZMm { get; set; }
    public float? SphereCentreX { get; set; }
    public float? SphereCentreY { get; set; }
    public float? SphereCentreZ { get; set; }
    public float? SphereRadius { get; set; }
}
