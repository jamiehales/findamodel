namespace findamodel.Services;

public interface IPlateSliceBitmapGenerator
{
    PngSliceExportMethod Method { get; }

    SliceBitmap RenderLayerBitmap(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm = PlateSliceRasterService.DefaultLayerHeightMm);
}

public interface IBatchPlateSliceBitmapGenerator : IPlateSliceBitmapGenerator
{
    IReadOnlyList<SliceBitmap> RenderLayerBitmaps(
        IReadOnlyList<IReadOnlyList<Triangle3D>> trianglesByLayer,
        IReadOnlyList<float> sliceHeightsMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight,
        float layerThicknessMm = PlateSliceRasterService.DefaultLayerHeightMm);
}
