namespace findamodel.Models;

public enum PrintingListMutateResult { Success, NotFound, IsDefault }

public record PrintingListSummaryDto(
    Guid Id,
    string Name,
    bool IsActive,
    bool IsDefault,
    string SpawnType,
    string HullMode,
    DateTime CreatedAt,
    string? OwnerUsername,
    int ItemCount);

public record PrintingListPrinterInfoDto(
    Guid Id,
    string Name,
    float BedWidthMm,
    float BedDepthMm);

public record PrintingListItemDto(
    Guid Id,
    Guid ModelId,
    int Quantity);

public record PrintingListDetailDto(
    Guid Id,
    string Name,
    bool IsActive,
    bool IsDefault,
    string SpawnType,
    string HullMode,
    DateTime CreatedAt,
    string? OwnerUsername,
        List<PrintingListItemDto> Items,
        PrintingListPrinterInfoDto? Printer);

public record CreatePrintingListRequest(string Name);

public record RenamePrintingListRequest(string Name);

public record UpdatePrintingListSettingsRequest(string SpawnType, string HullMode);

public record UpdatePrintingListPrinterRequest(Guid? PrinterConfigId);

public record UpsertPrintingListItemRequest(int Quantity);

public record PrintingListArchiveJobDto(
    Guid JobId,
    string FileName,
    string Status,
    int TotalEntries,
    int CompletedEntries,
    int ProgressPercent,
    string? CurrentEntryName,
    string? ErrorMessage);
