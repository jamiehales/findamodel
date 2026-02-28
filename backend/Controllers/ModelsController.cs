using Microsoft.AspNetCore.Mvc;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class ModelsController(ModelService modelService, ModelLoaderService loaderService, IConfiguration config) : ControllerBase
{
    [HttpGet]
    public async Task<IActionResult> GetModels()
    {
        var models = await modelService.GetModelsAsync();
        return Ok(models);
    }

    [HttpGet("{id:guid}/file")]
    public async Task<IActionResult> GetFile(Guid id)
    {
        var model = await modelService.GetModelAsync(id);
        if (model == null) return NotFound();

        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath)) return StatusCode(500);

        var fullPath = string.IsNullOrEmpty(model.Directory)
            ? Path.Combine(modelsPath, model.FileName)
            : Path.Combine(modelsPath, model.Directory, model.FileName);

        if (!System.IO.File.Exists(fullPath)) return NotFound();

        var contentType = model.FileType switch
        {
            "stl" => "model/stl",
            "obj" => "model/obj",
            _ => "application/octet-stream"
        };

        return PhysicalFile(fullPath, contentType, model.FileName);
    }

    [HttpGet("{id:guid}/preview")]
    public async Task<IActionResult> GetPreview(Guid id)
    {
        var png = await modelService.GetPreviewImageAsync(id);
        if (png == null) return NotFound();
        return File(png, "image/png");
    }

    /// <summary>
    /// Returns pre-processed geometry for a model: Y-up, mm scale, centred (X/Z at origin, base at Y=0).
    /// Response contains flat float arrays ready for THREE.BufferGeometry (positions and normals, 3 components each).
    /// </summary>
    [HttpGet("{id:guid}/geometry")]
    public async Task<IActionResult> GetGeometry(Guid id)
    {
        var model = await modelService.GetModelAsync(id);
        if (model == null) return NotFound();

        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath)) return StatusCode(500);

        var fullPath = string.IsNullOrEmpty(model.Directory)
            ? Path.Combine(modelsPath, model.FileName)
            : Path.Combine(modelsPath, model.Directory, model.FileName);

        if (!System.IO.File.Exists(fullPath)) return NotFound();

        var geometry = await loaderService.LoadModelAsync(fullPath, model.FileType);
        if (geometry == null) return StatusCode(500, "Failed to load model geometry");

        // Build flat float arrays for Three.js BufferGeometry.
        // 9 floats per triangle (3 vertices × 3 components each).
        var positions = new float[geometry.Triangles.Count * 9];
        var normals   = new float[geometry.Triangles.Count * 9];

        for (int i = 0; i < geometry.Triangles.Count; i++)
        {
            var tri = geometry.Triangles[i];
            int b = i * 9;
            positions[b + 0] = tri.V0.X; positions[b + 1] = tri.V0.Y; positions[b + 2] = tri.V0.Z;
            positions[b + 3] = tri.V1.X; positions[b + 4] = tri.V1.Y; positions[b + 5] = tri.V1.Z;
            positions[b + 6] = tri.V2.X; positions[b + 7] = tri.V2.Y; positions[b + 8] = tri.V2.Z;
            // Flat shading: same face normal repeated for all 3 vertices of the triangle
            normals[b + 0] = tri.Normal.X; normals[b + 1] = tri.Normal.Y; normals[b + 2] = tri.Normal.Z;
            normals[b + 3] = tri.Normal.X; normals[b + 4] = tri.Normal.Y; normals[b + 5] = tri.Normal.Z;
            normals[b + 6] = tri.Normal.X; normals[b + 7] = tri.Normal.Y; normals[b + 8] = tri.Normal.Z;
        }

        return Ok(new
        {
            positions,
            normals,
            triangleCount = geometry.Triangles.Count,
            sphereRadius  = geometry.SphereRadius,
            sphereCentre  = new { geometry.SphereCentre.X, geometry.SphereCentre.Y, geometry.SphereCentre.Z },
            dimensionXMm  = geometry.DimensionXMm,
            dimensionYMm  = geometry.DimensionYMm,
            dimensionZMm  = geometry.DimensionZMm,
        });
    }
}
