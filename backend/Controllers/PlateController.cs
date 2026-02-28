using Microsoft.AspNetCore.Mvc;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class PlateController : ControllerBase
{
    /// <summary>
    /// Generates a build plate STL from the supplied model placements (positions in mm, angle in radians).
    /// Currently returns an empty STL after a short processing delay.
    /// </summary>
    [HttpPost("generate")]
    public async Task<IActionResult> GeneratePlate([FromBody] GeneratePlateRequest request)
    {
        await Task.Delay(5000);

        // Empty binary STL: 80-byte header + 4-byte triangle count (0) = 84 bytes
        var stl = new byte[84];
        var header = System.Text.Encoding.ASCII.GetBytes("Empty plate STL");
        Buffer.BlockCopy(header, 0, stl, 0, header.Length);
        // Triangle count stays 0 (bytes 80-83 are already zero)

        return File(stl, "model/stl", "plate.stl");
    }
}

/// <param name="ModelId">GUID of the model.</param>
/// <param name="InstanceIndex">0-based index when the same model appears multiple times.</param>
/// <param name="XMm">X position on the plate in mm (origin = top-left corner of the plate).</param>
/// <param name="YMm">Y position on the plate in mm.</param>
/// <param name="AngleRad">Rotation angle in radians (counter-clockwise positive).</param>
public record PlacementDto(string ModelId, int InstanceIndex, double XMm, double YMm, double AngleRad);

public record GeneratePlateRequest(IReadOnlyList<PlacementDto> Placements);
