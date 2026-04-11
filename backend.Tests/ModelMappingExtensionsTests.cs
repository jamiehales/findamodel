using findamodel.Data.Entities;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ModelMappingExtensionsTests
{
    // ── Helpers ───────────────────────────────────────────────────────────────

    private static CachedModel FullModel() => new()
    {
        Id = Guid.Parse("11111111-1111-1111-1111-111111111111"),
        FileName = "dragon.stl",
        Directory = "Fantasy/Creatures",
        FileType = "stl",
        FileSize = 1024,
        PreviewImagePath = "preview/abc.png",
        CalculatedCreator = "Alice",
        CalculatedCollection = "Fantasy",
        CalculatedSubcollection = "Creatures",
        CalculatedCategory = "miniature",
        CalculatedType = "whole",
        CalculatedMaterial = "resin",
        CalculatedSupported = true,
        CalculatedModelName = "Dragon",
        ConvexHullCoordinates = "[[0,0],[1,0],[1,1]]",
        ConcaveHullCoordinates = "[[0,0],[0.5,0.1],[1,0],[1,1]]",
        ConvexSansRaftHullCoordinates = "[[0,0],[1,0]]",
        HullRaftHeightMm = 2f,
        DimensionXMm = 50f,
        DimensionYMm = 30f,
        DimensionZMm = 20f,
        SphereCentreX = 0f,
        SphereCentreY = 15f,
        SphereCentreZ = 0f,
        SphereRadius = 25f,
    };

    // ── ToModelDto ────────────────────────────────────────────────────────────

    [Fact]
    public void ToModelDto_MapsId()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal(Guid.Parse("11111111-1111-1111-1111-111111111111"), dto.Id);
    }

    [Fact]
    public void ToModelDto_UsesCalculatedModelName_WhenAvailable()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("Dragon", dto.Name);
    }

    [Fact]
    public void ToModelDto_FallsBackToFilenameWithoutExtension_WhenNoCalculatedModelName()
    {
        var model = FullModel();
        model.CalculatedModelName = null;
        model.FileName = "my_dragon.stl";
        var dto = model.ToModelDto();
        Assert.Equal("my_dragon", dto.Name);
    }

    [Fact]
    public void ToModelDto_BuildsRelativePath_ForSubdirectory()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("Fantasy/Creatures/dragon.stl", dto.RelativePath);
    }

    [Fact]
    public void ToModelDto_BuildsRelativePath_ForRootDirectory()
    {
        var model = FullModel();
        model.Directory = "";
        var dto = model.ToModelDto();
        Assert.Equal("dragon.stl", dto.RelativePath);
    }

    [Fact]
    public void ToModelDto_MapsFileType()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("stl", dto.FileType);
    }

    [Fact]
    public void ToModelDto_MapsFileSize()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal(1024, dto.FileSize);
    }

    [Fact]
    public void ToModelDto_BuildsFileUrl()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("/api/models/11111111-1111-1111-1111-111111111111/file", dto.FileUrl);
    }

    [Fact]
    public void ToModelDto_SetsHasPreviewTrue_WhenPreviewImagePathSet()
    {
        var dto = FullModel().ToModelDto();
        Assert.True(dto.HasPreview);
    }

    [Fact]
    public void ToModelDto_BuildsPreviewUrl_WhenPreviewImagePathSet()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("/api/models/11111111-1111-1111-1111-111111111111/preview", dto.PreviewUrl);
    }

    [Fact]
    public void ToModelDto_SetsHasPreviewFalse_WhenNoPreviewImagePath()
    {
        var model = FullModel();
        model.PreviewImagePath = null;
        var dto = model.ToModelDto();
        Assert.False(dto.HasPreview);
        Assert.Null(dto.PreviewUrl);
    }

    [Fact]
    public void ToModelDto_MapsMetadataFields()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("Alice", dto.Creator);
        Assert.Equal("Fantasy", dto.Collection);
        Assert.Equal("Creatures", dto.Subcollection);
        Assert.Equal("miniature", dto.Category);
        Assert.Equal("whole", dto.Type);
        Assert.Equal("resin", dto.Material);
        Assert.True(dto.Supported);
    }

    [Fact]
    public void ToModelDto_MapsHullCoordinates()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal("[[0,0],[1,0],[1,1]]", dto.ConvexHull);
        Assert.Equal("[[0,0],[0.5,0.1],[1,0],[1,1]]", dto.ConcaveHull);
        Assert.Equal("[[0,0],[1,0]]", dto.ConvexSansRaftHull);
    }

    [Fact]
    public void ToModelDto_MapsRaftHeightMm_WhenStored()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal(2f, dto.RaftHeightMm);
    }

    [Fact]
    public void ToModelDto_UsesDefaultRaftHeight_WhenNullOnEntity()
    {
        var model = FullModel();
        model.HullRaftHeightMm = null;
        var dto = model.ToModelDto();
        Assert.Equal(HullCalculationService.DefaultRaftHeightMm, dto.RaftHeightMm);
    }

    [Fact]
    public void ToModelDto_MapsDimensions()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal(50f, dto.DimensionXMm);
        Assert.Equal(30f, dto.DimensionYMm);
        Assert.Equal(20f, dto.DimensionZMm);
    }

    [Fact]
    public void ToModelDto_MapsSphereCentreAndRadius()
    {
        var dto = FullModel().ToModelDto();
        Assert.Equal(0f, dto.SphereCentreX);
        Assert.Equal(15f, dto.SphereCentreY);
        Assert.Equal(0f, dto.SphereCentreZ);
        Assert.Equal(25f, dto.SphereRadius);
    }

    // ── ApplyCalculatedMetadata ───────────────────────────────────────────────

    [Fact]
    public void ApplyCalculatedMetadata_SetsAllFields()
    {
        var entity = new CachedModel();
        var metadata = new ModelMetadataHelper.ComputedMetadata
        {
            Creator = "Bob",
            Collection = "SciFi",
            Subcollection = "Robots",
            Category = "bust",
            Type = "part",
            Material = "fdm",
            Supported = false,
            ModelName = "Robot Head",
        };
        entity.ApplyCalculatedMetadata(metadata);

        Assert.Equal("Bob", entity.CalculatedCreator);
        Assert.Equal("SciFi", entity.CalculatedCollection);
        Assert.Equal("Robots", entity.CalculatedSubcollection);
        Assert.Equal("bust", entity.CalculatedCategory);
        Assert.Equal("part", entity.CalculatedType);
        Assert.Equal("fdm", entity.CalculatedMaterial);
        Assert.False(entity.CalculatedSupported);
        Assert.Equal("Robot Head", entity.CalculatedModelName);
    }

    [Fact]
    public void ApplyCalculatedMetadata_ClearsFields_WhenAllNullMetadata()
    {
        var entity = new CachedModel
        {
            CalculatedCreator = "old creator",
            CalculatedCollection = "old collection",
        };
        entity.ApplyCalculatedMetadata(new ModelMetadataHelper.ComputedMetadata());

        Assert.Null(entity.CalculatedCreator);
        Assert.Null(entity.CalculatedCollection);
    }
}
