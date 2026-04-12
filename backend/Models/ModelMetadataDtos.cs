namespace findamodel.Models;

/// <summary>
/// Plain-value per-model metadata overrides stored under findamodel.yaml:model_metadata.
/// All fields are optional; null means no override for that field.
/// </summary>
public record UpdateModelMetadataRequest(
    string? Name,
    string? PartName,
    string? Creator,
    string? Collection,
    string? Subcollection,
    List<string>? Tags,
    string? Category,
    string? Type,
    string? Material,
    bool? Supported,
    float? RaftHeightMm);

/// <summary>
/// Returned by GET /api/models/{id}/metadata. Contains the per-model overrides (localValues)
/// and the folder-resolved values a model inherits from its directory config (inheritedValues).
/// </summary>
public record ModelMetadataDetail(
    ModelMetadataEntry LocalValues,
    ModelMetadataEntry? InheritedValues);
