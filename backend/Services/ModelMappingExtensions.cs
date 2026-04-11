using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

internal static class ModelMappingExtensions
{
    public static ModelDto ToModelDto(this CachedModel model)
    {
        return new ModelDto
        {
            Id = model.Id,
            Name = model.CalculatedModelName ?? Path.GetFileNameWithoutExtension(model.FileName),
            RelativePath = ComputeRelativePath(model.Directory, model.FileName),
            FileType = model.FileType,
            FileSize = model.FileSize,
            FileUrl = $"/api/models/{model.Id}/file",
            HasPreview = model.PreviewImagePath != null,
            PreviewUrl = model.PreviewImagePath != null ? $"/api/models/{model.Id}/preview?v={ModelPreviewService.CurrentPreviewGenerationVersion}" : null,
            Creator = model.CalculatedCreator,
            Collection = model.CalculatedCollection,
            Subcollection = model.CalculatedSubcollection,
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
        entity.CalculatedCategory = metadata.Category;
        entity.CalculatedType = metadata.Type;
        entity.CalculatedMaterial = metadata.Material;
        entity.CalculatedSupported = metadata.Supported;
        entity.CalculatedModelName = metadata.ModelName;
    }

    private static string ComputeRelativePath(string directory, string fileName) =>
        string.IsNullOrEmpty(directory) ? fileName : $"{directory}/{fileName}";
}
