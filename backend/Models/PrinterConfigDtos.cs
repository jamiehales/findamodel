namespace findamodel.Models;

public record PrinterConfigDto(
    Guid Id,
    string Name,
    float BedWidthMm,
    float BedDepthMm,
    bool IsBuiltIn,
    bool IsDefault);

public record CreatePrinterConfigRequest(string Name, float BedWidthMm, float BedDepthMm);

public record UpdatePrinterConfigRequest(string Name, float BedWidthMm, float BedDepthMm);
