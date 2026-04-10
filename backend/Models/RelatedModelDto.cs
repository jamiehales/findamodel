namespace findamodel.Models;

public record RelatedModelDto(
    Guid Id,
    string Name,
    string RelativePath,
    string FileType,
    long FileSize,
    string? PreviewUrl);
