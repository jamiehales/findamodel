using findamodel.Data.Entities;
using findamodel.Models;
using System.Text.Json;

namespace findamodel.Services;

internal static class ModelMappingExtensions
{
    public static ModelDto ToModelDto(this CachedModel model)
    {
        return new ModelDto
        {
            Id = model.Id,
            Name = model.CalculatedModelName ?? Path.GetFileNameWithoutExtension(model.FileName),
            PartName = model.CalculatedPartName,
            RelativePath = ComputeRelativePath(model.Directory, model.FileName),
            FileType = model.FileType,
            CanExportToPlate = model.FileType is "stl" or "obj",
            FileSize = model.FileSize,
            FileUrl = $"/api/models/{model.Id}/file",
            HasPreview = model.PreviewImagePath != null,
            PreviewUrl = model.PreviewImagePath != null ? $"/api/models/{model.Id}/preview?v={model.PreviewGenerationVersion ?? 0}" : null,
            Creator = model.CalculatedCreator,
            Collection = model.CalculatedCollection,
            Subcollection = model.CalculatedSubcollection,
            Tags = TagListHelper.FromJson(model.CalculatedTagsJson),
            GeneratedTags = TagListHelper.FromJson(model.GeneratedTagsJson),
            GeneratedTagConfidence = ParseConfidenceJson(model.GeneratedTagsConfidenceJson),
            GeneratedTagsStatus = model.GeneratedTagsStatus ?? "none",
            GeneratedTagsAt = model.GeneratedTagsAt,
            GeneratedTagsError = model.GeneratedTagsError,
            GeneratedTagsModel = model.GeneratedTagsModel,
            Category = model.CalculatedCategory,
            Type = model.CalculatedType,
            Material = model.CalculatedMaterial,
            Supported = model.CalculatedSupported,
            ConvexHull = model.ConvexHullCoordinates,
            ConcaveHull = model.ConcaveHullCoordinates,
            ConvexSansRaftHull = model.ConvexSansRaftHullCoordinates,
            RaftHeightMm = model.HullRaftHeightMm ?? HullCalculationService.DefaultRaftHeightMm,
            DimensionXMm = model.DimensionXMm,
            DimensionYMm = model.DimensionYMm,
            DimensionZMm = model.DimensionZMm,
            SphereCentreX = model.SphereCentreX,
            SphereCentreY = model.SphereCentreY,
            SphereCentreZ = model.SphereCentreZ,
            SphereRadius = model.SphereRadius,
        };
    }

    public static void ApplyCalculatedMetadata(this CachedModel entity, ModelMetadataHelper.ComputedMetadata metadata)
    {
        entity.CalculatedCreator = metadata.Creator;
        entity.CalculatedCollection = metadata.Collection;
        entity.CalculatedSubcollection = metadata.Subcollection;
        entity.CalculatedTagsJson = TagListHelper.ToJsonOrNull(metadata.Tags);
        entity.CalculatedCategory = metadata.Category;
        entity.CalculatedType = metadata.Type;
        entity.CalculatedMaterial = metadata.Material;
        entity.CalculatedSupported = metadata.Supported;
        entity.CalculatedModelName = metadata.ModelName;
        entity.CalculatedPartName = metadata.PartName;
    }

    private static string ComputeRelativePath(string directory, string fileName) =>
        string.IsNullOrEmpty(directory) ? fileName : $"{directory}/{fileName}";

    private static Dictionary<string, float> ParseConfidenceJson(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
            return new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);

        try
        {
            return JsonSerializer.Deserialize<Dictionary<string, float>>(json)
                ?? new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        }
        catch
        {
            return new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        }
    }
}
