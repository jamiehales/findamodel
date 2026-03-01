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
    public string? RawAuthor { get; set; }
    public string? RawCollection { get; set; }
    public string? RawCategory { get; set; }   // "Bust" | "Miniature" | "Uncategorized" | null
    public string? RawType { get; set; }        // "Whole" | "Part" | null
    public bool? RawSupported { get; set; }
    public string? RawSubcollection { get; set; }

    // Resolved (composed) values — computed at scan time by walking ancestors; closest non-null wins
    public string? Author { get; set; }
    public string? Collection { get; set; }
    public string? Subcollection { get; set; }
    public string? Category { get; set; }
    public string? Type { get; set; }
    public bool? Supported { get; set; }

    // SHA256 hash of the findamodel.yaml at this directory (null = no config file present)
    public string? LocalConfigFileHash { get; set; }
    public DateTime UpdatedAt { get; set; }

    public List<CachedModel> Models { get; set; } = [];
}
