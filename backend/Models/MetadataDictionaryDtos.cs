namespace findamodel.Models;

public record MetadataDictionaryValueDto(Guid Id, string Value);

public record MetadataDictionaryFieldDto(
    List<MetadataDictionaryValueDto> Configured,
    List<string> Observed);

public record MetadataDictionaryOverviewDto(
    MetadataDictionaryFieldDto Category,
    MetadataDictionaryFieldDto Type,
    MetadataDictionaryFieldDto Material,
    MetadataDictionaryFieldDto Tags);

public record CreateMetadataDictionaryValueRequest(string Field, string Value);

public record UpdateMetadataDictionaryValueRequest(string Value);
