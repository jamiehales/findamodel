namespace findamodel.Models;

public record GeneratedTagsResultDto(
    Guid ModelId,
    string Status,
    List<string> GeneratedTags,
    Dictionary<string, float> Confidence,
    string? Error,
    DateTime? UpdatedAt,
    string? Provider,
    string? Model);

public record AcceptGeneratedTagsRequest(List<string>? Tags);
