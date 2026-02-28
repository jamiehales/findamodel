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
}
