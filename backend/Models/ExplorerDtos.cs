namespace findamodel.Models;

/// <summary>Request body for creating or replacing a directory's local metadata config.</summary>
public record UpdateDirectoryConfigRequest(
    string? Creator,
    string? Collection,
    string? Subcollection,
    string? Category,
    string? Type,
    bool? Supported);

/// <summary>A snapshot of raw or resolved metadata fields for a directory.</summary>
public record ConfigFieldsDto(
    string? Creator,
    string? Collection,
    string? Subcollection,
    string? Category,
    string? Type,
    bool? Supported);

/// <summary>
/// Full config detail for a directory, including the local (raw) values and the
/// parent's resolved values so the UI can show inherited placeholders.
/// </summary>
public record DirectoryConfigDetailDto(
    string DirectoryPath,
    ConfigFieldsDto LocalValues,
    ConfigFieldsDto? ParentResolvedValues,
    string? ParentPath);

/// <summary>A folder entry in the explorer grid.</summary>
public record FolderItemDto(
    string Name,
    string Path,
    int SubdirectoryCount,
    int ModelCount,
    ConfigFieldsDto ResolvedValues);

/// <summary>
/// A model-file entry in the explorer grid.
/// Id is null when the file has not yet been indexed.
/// </summary>
public record ExplorerModelItemDto(
    string? Id,
    string FileName,
    string RelativePath,
    string FileType,
    long? FileSize,
    bool HasPreview,
    string? PreviewUrl);

/// <summary>Response for GET /api/explorer.</summary>
public record ExplorerResponseDto(
    string CurrentPath,
    string? ParentPath,
    List<FolderItemDto> Folders,
    List<ExplorerModelItemDto> Models);

