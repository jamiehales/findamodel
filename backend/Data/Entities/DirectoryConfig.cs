namespace findamodel.Data.Entities;

public class DirectoryConfig
{
    public Guid Id { get; set; }
    public string DirectoryPath { get; set; } = "";  // same convention as CachedModel.Directory: forward slashes, "" = root

    // Self-referential parent link (null for root)
    public Guid? ParentId { get; set; }
    public DirectoryConfig? Parent { get; set; }
    public List<DirectoryConfig> Children { get; set; } = [];

    // Raw (local) values — sourced exclusively from THIS directory's findamodel.yaml
    public string? RawCreator { get; set; }
    public string? RawCollection { get; set; }
    public string? RawCategory { get; set; }
    public string? RawType { get; set; }
    public string? RawMaterial { get; set; }
    public bool? RawSupported { get; set; }
    public string? RawSubcollection { get; set; }
    public string? RawModelName { get; set; }
    public float? RawRaftHeightMm { get; set; }

    // Resolved (composed) values — computed at scan time by walking ancestors; closest non-null wins
    public string? Creator { get; set; }
    public string? Collection { get; set; }
    public string? Subcollection { get; set; }
    public string? ModelName { get; set; }
    public string? Category { get; set; }
    public string? Type { get; set; }
    public string? Material { get; set; }
    public bool? Supported { get; set; }
    public float? RaftHeightMm { get; set; }

    // Rule definitions stored as YAML: Dictionary<fieldName, ruleConfigObject>
    // e.g. "creator:\n  rule: filename\n  include_extension: false\n"
    // Raw = defined in THIS directory's YAML; Resolved = after inheritance walk
    public string? RawRulesYaml { get; set; }
    public string? ResolvedRulesYaml { get; set; }

    // SHA256 hash of the findamodel.yaml at this directory (null = no config file present)
    public string? LocalConfigFileHash { get; set; }
    public DateTime UpdatedAt { get; set; }

    /// <summary>
    /// JSON-serialized Dictionary&lt;string, ModelMetadataEntry&gt; parsed from this directory's
    /// findamodel.yaml model_metadata section.  Key = filename (case-insensitive on lookup).
    /// Null means the section was absent.
    /// </summary>
    public string? RawModelMetadataJson { get; set; }

    public List<CachedModel> Models { get; set; } = [];
}
