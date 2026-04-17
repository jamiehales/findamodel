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
