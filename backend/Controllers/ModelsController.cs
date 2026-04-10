using Microsoft.AspNetCore.Mvc;
using findamodel.Models;
using findamodel.Services;
using System.Text.Json;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class ModelsController(
    ModelService modelService,
    ModelLoaderService loaderService,
    MeshTransferService meshTransferService,
    IConfiguration config) : ControllerBase
{
    [HttpGet]
    public async Task<IActionResult> GetModels([FromQuery] int? limit = null)
    {
        var models = await modelService.GetModelsAsync(limit);
        return Ok(models);
    }

    [HttpGet("{id:guid}")]
    public async Task<IActionResult> GetModel(Guid id)
    {
        var model = await modelService.GetModelDtoAsync(id);
        if (model == null) return NotFound();
        return Ok(model);
    }

    [HttpGet("{id:guid}/other-parts")]
    public async Task<ActionResult<List<RelatedModelDto>>> GetOtherParts(Guid id)
    {
        var parts = await modelService.GetOtherPartsAsync(id);
        return Ok(parts);
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
        var previewPath = await modelService.GetPreviewImagePathAsync(id);
        if (string.IsNullOrEmpty(previewPath)) return NotFound();

        // Compute cache path the same way as Program.cs
        var dataPath = config["Configuration:DataPath"] ?? "data";
        var resolvedDataPath = Path.GetFullPath(dataPath);
        var cacheRendersPath = Path.Combine(resolvedDataPath, "cache", "renders");

        var fullPath = Path.Combine(cacheRendersPath, previewPath);
        if (!System.IO.File.Exists(fullPath)) return NotFound();

        return PhysicalFile(fullPath, "image/png");
    }

    /// <summary>
    /// Returns pre-processed geometry for a model: Y-up, mm scale, centred (X/Z at origin, base at Y=0).
    /// Binary response contains indexed, 16-bit quantised positions plus 16/32-bit triangle indices.
    /// Legacy JSON remains available for callers that do not request the mesh mime type.
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

        if (!ClientPrefersBinaryMesh(Request))
            return new JsonResult(new
            {
                positions = BuildLegacyPositions(geometry),
                normals = BuildLegacyNormals(geometry),
                triangleCount = geometry.Triangles.Count,
                sphereRadius  = geometry.SphereRadius,
                sphereCentre  = new { geometry.SphereCentre.X, geometry.SphereCentre.Y, geometry.SphereCentre.Z },
                dimensionXMm  = geometry.DimensionXMm,
                dimensionYMm  = geometry.DimensionYMm,
                dimensionZMm  = geometry.DimensionZMm
            });

        var payload = meshTransferService.Encode(geometry);
        return File(payload, MeshTransferService.ContentType);
    }

    private static bool ClientPrefersBinaryMesh(HttpRequest request)
    {
        if (string.Equals(request.Query["format"], "json", StringComparison.OrdinalIgnoreCase))
            return false;

        var accept = request.Headers.Accept.ToString();
        return accept.Contains(MeshTransferService.ContentType, StringComparison.OrdinalIgnoreCase);
    }

    private static float[] BuildLegacyPositions(LoadedGeometry geometry)
    {
        var positions = new float[geometry.Triangles.Count * 9];
        for (int i = 0; i < geometry.Triangles.Count; i++)
        {
            var tri = geometry.Triangles[i];
            int b = i * 9;
            positions[b + 0] = tri.V0.X; positions[b + 1] = tri.V0.Y; positions[b + 2] = tri.V0.Z;
            positions[b + 3] = tri.V1.X; positions[b + 4] = tri.V1.Y; positions[b + 5] = tri.V1.Z;
            positions[b + 6] = tri.V2.X; positions[b + 7] = tri.V2.Y; positions[b + 8] = tri.V2.Z;
        }

        return positions;
    }

    private static float[] BuildLegacyNormals(LoadedGeometry geometry)
    {
        var normals = new float[geometry.Triangles.Count * 9];
        for (int i = 0; i < geometry.Triangles.Count; i++)
        {
            var tri = geometry.Triangles[i];
            int b = i * 9;
            normals[b + 0] = tri.Normal.X; normals[b + 1] = tri.Normal.Y; normals[b + 2] = tri.Normal.Z;
            normals[b + 3] = tri.Normal.X; normals[b + 4] = tri.Normal.Y; normals[b + 5] = tri.Normal.Z;
            normals[b + 6] = tri.Normal.X; normals[b + 7] = tri.Normal.Y; normals[b + 8] = tri.Normal.Z;
        }

        return normals;
    }
}
