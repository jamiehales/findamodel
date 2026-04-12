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
    public string? PreviewImagePath { get; set; }  // Path to PNG preview render relative to cache/renders folder (null = not yet generated)
    public DateTime? PreviewGeneratedAt { get; set; }
    public int? PreviewGenerationVersion { get; set; }  // Version of preview rendering algorithm (null = not yet generated or version unknown)

    // FK to the DirectoryConfig record for this model's containing directory
    public Guid? DirectoryConfigId { get; set; }
    public DirectoryConfig? DirectoryConfig { get; set; }

    // Cached computed metadata — DirectoryConfig resolved values with rules applied to this specific file
    public string? CalculatedCreator { get; set; }
    public string? CalculatedCollection { get; set; }
    public string? CalculatedSubcollection { get; set; }
    public string? CalculatedTagsJson { get; set; }
    public string? CalculatedCategory { get; set; }
    public string? CalculatedType { get; set; }
    public string? CalculatedMaterial { get; set; }
    public bool? CalculatedSupported { get; set; }
    public string? CalculatedModelName { get; set; }
    public string? CalculatedPartName { get; set; }

    // Hull coordinates as JSON arrays: [[x1,y1],[x2,y2],...]
    // Projected onto X-Z plane with Y-up coordinate system (bird's eye view)
    public string? ConvexHullCoordinates { get; set; }           // Outer boundary hull (JSON)
    public string? ConcaveHullCoordinates { get; set; }          // Concave/alpha hull (JSON)
    public string? ConvexSansRaftHullCoordinates { get; set; }   // Convex hull excluding vertices below Y=2mm (JSON)
    public int? HullGenerationVersion { get; set; }              // Version of hull generation algorithm/settings
    public float? HullRaftHeightMm { get; set; }                 // Raft cutoff used when generating hulls
    public string? ScanConfigChecksum { get; set; }              // Hash of all config inputs that affect hull generation (null = needs regeneration)
    public DateTime? HullGeneratedAt { get; set; }

    // Geometry metadata — Y-up, mm, centred (X/Z at origin, base at Y=0)
    public float? DimensionXMm { get; set; }
    public float? DimensionYMm { get; set; }
    public float? DimensionZMm { get; set; }
    public float? SphereCentreX { get; set; }
    public float? SphereCentreY { get; set; }
    public float? SphereCentreZ { get; set; }
    public float? SphereRadius { get; set; }
    public DateTime? GeometryCalculatedAt { get; set; }
}
