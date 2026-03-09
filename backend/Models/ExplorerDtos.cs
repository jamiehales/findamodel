namespace findamodel.Models;

/// <summary>Request body for creating or replacing a directory's local metadata config.</summary>
public record UpdateDirectoryConfigRequest(
    string? Creator,
    string? Collection,
    string? Subcollection,
    string? Category,
    string? Type,
    bool? Supported,
    string? ModelName = null);

/// <summary>A snapshot of raw or resolved metadata fields for a directory.</summary>
public record ConfigFieldsDto(
    string? Creator,
    string? Collection,
    string? Subcollection,
    string? Category,
    string? Type,
    bool? Supported,
    string? ModelName = null);

/// <summary>
/// Full config detail for a directory, including the local (raw) values and the
/// parent's resolved values so the UI can show inherited placeholders.
/// LocalRuleFields contains field names whose values in this directory's YAML are rule
/// definitions rather than plain values (e.g. "creator", "model_name").
/// </summary>
public record DirectoryConfigDetailDto(
    string DirectoryPath,
    ConfigFieldsDto LocalValues,
    ConfigFieldsDto? ParentResolvedValues,
    string? ParentPath,
    HashSet<string>? LocalRuleFields = null);

/// <summary>A folder entry in the explorer grid.</summary>
/// <param name="RuleConfigs">
/// Maps field names to their YAML rule snippet, for fields whose resolved value comes from an
/// inherited rule rather than a plain value. Null when no rules are in effect for this directory.
/// </param>
public record FolderItemDto(
    string Name,
    string Path,
    int SubdirectoryCount,
    int ModelCount,
    ConfigFieldsDto ResolvedValues,
    Dictionary<string, string>? RuleConfigs = null);

/// <summary>
/// A model-file entry in the explorer grid.
/// Id is null when the file has not yet been indexed.
/// ResolvedMetadata contains the effective metadata for this file (including rule-evaluated values).
/// RuleFields is the set of field names whose values were computed by a rule (not a plain value).
/// </summary>
public record ExplorerModelItemDto(
    string? Id,
    string FileName,
    string RelativePath,
    string FileType,
    long? FileSize,
    bool HasPreview,
    string? PreviewUrl,
    ConfigFieldsDto? ResolvedMetadata = null,
    Dictionary<string, string>? RuleConfigs = null);

/// <summary>Response for GET /api/explorer.</summary>
public record ExplorerResponseDto(
    string CurrentPath,
    string? ParentPath,
    List<FolderItemDto> Folders,
    List<ExplorerModelItemDto> Models);

