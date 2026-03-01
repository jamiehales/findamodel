using Microsoft.AspNetCore.Mvc;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class PlateController(
    ModelService modelService,
    ModelLoaderService loaderService,
    ModelSaverService saverService,
    IConfiguration config) : ControllerBase
{
    /// <summary>
    /// Generates a build-plate STL from the supplied model placements.
    ///
    /// Each placement specifies a model ID, its XY position on the plate (in mm), and a
    /// rotation angle (radians, counter-clockwise when viewed from above).
    ///
    /// Pipeline per model:
    ///   1. Load geometry via ModelLoaderService → Y-up, mm, centred (base at Y = 0).
    ///   2. Rotate around the Y axis by AngleRad.
    ///   3. Translate: X += XMm, Z += YMm  (plate XY maps to world XZ in Y-up space).
    ///   4. Convert to Z-up ((x,y,z) → (-x,z,y)) for slicer-compatible output.
    ///
    /// All transformed models are merged into a single binary STL and returned as a download.
    /// </summary>
    [HttpPost("generate")]
    public async Task<IActionResult> GeneratePlate([FromBody] GeneratePlateRequest request)
    {
        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath))
            return StatusCode(500, "Models:DirectoryPath not configured");

        var allTriangles = new List<Triangle3D>();

        foreach (var placement in request.Placements)
        {
            if (!Guid.TryParse(placement.ModelId, out var modelId))
                return BadRequest($"Invalid model ID: {placement.ModelId}");

            var model = await modelService.GetModelAsync(modelId);
            if (model == null)
                return NotFound($"Model not found: {placement.ModelId}");

            var fullPath = string.IsNullOrEmpty(model.Directory)
                ? Path.Combine(modelsPath, model.FileName)
                : Path.Combine(modelsPath, model.Directory, model.FileName);

            if (!System.IO.File.Exists(fullPath))
                return NotFound($"Model file not found on disk: {model.FileName}");

            var geometry = await loaderService.LoadModelAsync(fullPath, model.FileType);
            if (geometry == null)
                return StatusCode(500, $"Failed to parse geometry for: {model.FileName}");

            // Pre-compute rotation coefficients
            float sinA = MathF.Sin((float)placement.AngleRad);
            float cosA = MathF.Cos((float)placement.AngleRad);

            foreach (var tri in geometry.Triangles)
            {
                allTriangles.Add(new Triangle3D(
                    PlaceVertex(tri.V0, sinA, cosA, (float)placement.XMm, (float)placement.YMm),
                    PlaceVertex(tri.V1, sinA, cosA, (float)placement.XMm, (float)placement.YMm),
                    PlaceVertex(tri.V2, sinA, cosA, (float)placement.XMm, (float)placement.YMm),
                    RotateY(tri.Normal, sinA, cosA)));
            }
        }

        // Convert Y-up → Z-up: (x,y,z) → (-x,z,y) — slicers expect Z-up
        var zUpTriangles = new List<Triangle3D>(allTriangles.Count);
        foreach (var tri in allTriangles)
        {
            zUpTriangles.Add(new Triangle3D(
                YUpToZUp(tri.V0),
                YUpToZUp(tri.V1),
                YUpToZUp(tri.V2),
                YUpToZUp(tri.Normal)));
        }

        var stlBytes = saverService.SaveStl(zUpTriangles, "findamodel plate");
        return File(stlBytes, "model/stl", "plate.stl");
    }

    /// <summary>Rotate around Y axis then translate in the XZ plane.
    /// Matches the 2D canvas convention: x' = x·cos − z·sin, z' = x·sin + z·cos
    /// (positive angle = CCW when viewed from above, consistent with Matter.js).
    /// </summary>
    private static Vec3 PlaceVertex(Vec3 v, float sinA, float cosA, float xMm, float yMm)
        => new(
            v.X * cosA - v.Z * sinA + xMm,
            v.Y,
            v.X * sinA + v.Z * cosA + yMm);

    /// <summary>Rotate around Y axis (normals are directions — no translation).</summary>
    private static Vec3 RotateY(Vec3 n, float sinA, float cosA)
        => new(
            n.X * cosA - n.Z * sinA,
            n.Y,
            n.X * sinA + n.Z * cosA);

    /// <summary>Y-up (x,y,z) → Z-up (-x,z,y). The ZUpToYUp transform is its own inverse.</summary>
    private static Vec3 YUpToZUp(Vec3 v) => new(-v.X, v.Z, v.Y);
}

/// <param name="ModelId">GUID of the model.</param>
/// <param name="InstanceIndex">0-based index when the same model appears multiple times on the plate.</param>
/// <param name="XMm">X position on the plate in mm.</param>
/// <param name="YMm">Y position on the plate in mm (maps to Z in 3D Y-up space).</param>
/// <param name="AngleRad">Rotation around the vertical axis in radians (counter-clockwise when viewed from above).</param>
public record PlacementDto(string ModelId, int InstanceIndex, double XMm, double YMm, double AngleRad);

public record GeneratePlateRequest(IReadOnlyList<PlacementDto> Placements);
