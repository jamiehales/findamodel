namespace findamodel.Models;

public class ModelDto
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public string? PartName { get; set; }
    public string RelativePath { get; set; } = "";
    public string FileType { get; set; } = "";
    public bool CanExportToPlate { get; set; }
    public long FileSize { get; set; }
    public string FileUrl { get; set; } = "";
    public bool HasPreview { get; set; }
    public string? PreviewUrl { get; set; }

    // Resolved metadata from DirectoryConfig ancestry
    public string? Creator { get; set; }
    public string? Collection { get; set; }
    public string? Subcollection { get; set; }
    public List<string> Tags { get; set; } = [];
    public List<string> GeneratedTags { get; set; } = [];
    public Dictionary<string, float> GeneratedTagConfidence { get; set; } = new(StringComparer.OrdinalIgnoreCase);
    public string GeneratedTagsStatus { get; set; } = "none";
    public DateTime? GeneratedTagsAt { get; set; }
    public string? GeneratedTagsError { get; set; }
    public string? GeneratedTagsModel { get; set; }
    public string? Category { get; set; }   // "Bust" | "Miniature" | "Uncategorized" | null
    public string? Type { get; set; }        // "Whole" | "Part" | null
    public string? Material { get; set; }    // "FDM" | "Resin" | "Any" | null
    public bool? Supported { get; set; }

    // Hull coordinates as JSON: [[x,z],[x,z],...] projected onto X-Z plane (bird's eye with Y-up)
    public string? ConvexHull { get; set; }
    public string? ConcaveHull { get; set; }
    public string? ConvexSansRaftHull { get; set; }  // Convex hull excluding vertices below RaftHeightMm
    public float RaftHeightMm { get; set; }          // Y cutoff used for sans-raft hull calculation

    // Geometry metadata (mm, Y-up centred coordinate system: X/Z at origin, base at Y=0)
    public float? DimensionXMm { get; set; }
    public float? DimensionYMm { get; set; }
    public float? DimensionZMm { get; set; }
    public float? SphereCentreX { get; set; }
    public float? SphereCentreY { get; set; }
    public float? SphereCentreZ { get; set; }
    public float? SphereRadius { get; set; }
}
