namespace findamodel.Models;

/// <param name="ModelId">GUID of the model.</param>
/// <param name="InstanceIndex">0-based index when the same model appears multiple times on the plate.</param>
/// <param name="XMm">X position on the plate in mm.</param>
/// <param name="YMm">Y position on the plate in mm (maps to Z in 3D Y-up space).</param>
/// <param name="AngleRad">Rotation around the vertical axis in radians (counter-clockwise when viewed from above).</param>
public record PlacementDto(string ModelId, int InstanceIndex, double XMm, double YMm, double AngleRad);

/// <param name="Format">Output format: 3mf (default), stl, or glb.</param>
public record GeneratePlateRequest(IReadOnlyList<PlacementDto> Placements, string? Format = null);

public record PlateGenerationJobDto(
    Guid JobId,
    string FileName,
    string Format,
    string Status,
    int TotalEntries,
    int CompletedEntries,
    int ProgressPercent,
    string? CurrentEntryName,
    string? ErrorMessage,
    string? Warning,
    IReadOnlyList<string> SkippedModels);
