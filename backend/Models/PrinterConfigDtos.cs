namespace findamodel.Models;

public record PrinterConfigDto(
    Guid Id,
    string Name,
    float BedWidthMm,
    float BedDepthMm,
    int PixelWidth,
    int PixelHeight,
    bool IsBuiltIn,
    bool IsDefault);

public record CreatePrinterConfigRequest(string Name, float BedWidthMm, float BedDepthMm, int PixelWidth, int PixelHeight);

public record UpdatePrinterConfigRequest(string Name, float BedWidthMm, float BedDepthMm, int PixelWidth, int PixelHeight);
