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
}
