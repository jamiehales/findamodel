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
    List<PrintingListItemDto> Items);

public record CreatePrintingListRequest(string Name);

public record RenamePrintingListRequest(string Name);

public record UpdatePrintingListSettingsRequest(string SpawnType, string HullMode);

public record UpsertPrintingListItemRequest(int Quantity);
