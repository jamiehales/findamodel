namespace findamodel.Models;

/// <summary>Per-model overrides defined in a findamodel.yaml model_metadata section.</summary>
public record ModelMetadataEntry(
    string? Name,
    string? PartName,
    string? Creator = null,
    string? Collection = null,
    string? Subcollection = null,
    List<string>? Tags = null,
    string? Category = null,
    string? Type = null,
    string? Material = null,
    bool? Supported = null,
    float? RaftHeightMm = null);
