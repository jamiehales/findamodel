using System.Buffers.Binary;
using Microsoft.AspNetCore.Mvc;
using findamodel.Models;
using findamodel.Services;
using System.Text.Json;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class ModelsController(
    ModelService modelService,
    TagGenerationService tagGenerationService,
    ModelLoaderService loaderService,
    MeshTransferService meshTransferService,
    SupportSeparationService supportSeparation,
    IConfiguration config) : ControllerBase
{
    [HttpGet]
    public async Task<IActionResult> GetModels([FromQuery] int? limit = null)
    {
        var models = await modelService.GetModelsAsync(limit);
        return Ok(models);
    }

    [HttpPost("by-ids")]
    public async Task<IActionResult> GetModelsByIds([FromBody] ModelsByIdsRequest? request)
    {
        if (request?.Ids == null)
            return BadRequest("ids is required.");

        var models = await modelService.GetModelsByIdsAsync(request.Ids);
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
    public async Task<IActionResult> GetPreview(Guid id, [FromQuery] bool? includeSupports = null)
    {
        var requestedIncludeSupports = includeSupports ?? true;
        var previewPaths = await modelService.GetPreviewImagePathCandidatesAsync(id, requestedIncludeSupports);
        if (previewPaths == null) return NotFound();

        // Compute cache path the same way as Program.cs
        var dataPath = config["Configuration:DataPath"] ?? "data";
        var resolvedDataPath = Path.GetFullPath(dataPath);
        var cacheRendersPath = Path.Combine(resolvedDataPath, "cache", "renders");

        string? selectedPreviewPath = null;
        var servedRequestedVariant = false;
        foreach (var candidate in previewPaths.EnumerateCandidates())
        {
            var candidateFullPath = Path.Combine(cacheRendersPath, candidate.RelativePath);
            if (!System.IO.File.Exists(candidateFullPath))
                continue;

            selectedPreviewPath = candidate.RelativePath;
            servedRequestedVariant = candidate.IsPreferred;
            break;
        }

        if (selectedPreviewPath == null) return NotFound();

        var fullPath = Path.Combine(cacheRendersPath, selectedPreviewPath);

        // Exact-variant URLs are immutable. Fallback responses remain short-lived because
        // the requested variant might be generated later under the same URL.
        Response.Headers.CacheControl = servedRequestedVariant
            ? "public, max-age=31536000, immutable"
            : "public, max-age=60";
        return PhysicalFile(fullPath, "image/png");
    }

    [HttpPut("{id:guid}/metadata")]
    public async Task<IActionResult> UpdateMetadata(Guid id, [FromBody] UpdateModelMetadataRequest request)
    {
        var model = await modelService.UpdateModelMetadataAsync(id, request);
        if (model == null) return NotFound();
        return Ok(model);
    }

    [HttpGet("{id:guid}/metadata")]
    public async Task<IActionResult> GetMetadata(Guid id)
    {
        var metadata = await modelService.GetModelMetadataAsync(id);
        if (metadata == null) return NotFound();
        return Ok(metadata);
    }

    [HttpGet("{id:guid}/tags/generated")]
    public async Task<ActionResult<GeneratedTagsResultDto>> GetGeneratedTags(Guid id, CancellationToken ct)
    {
        var result = await tagGenerationService.GetGeneratedTagsAsync(id, ct);
        if (result == null) return NotFound();
        return Ok(result);
    }

    [HttpPost("{id:guid}/tags/generate")]
    public async Task<ActionResult<GeneratedTagsResultDto>> GenerateTags(Guid id, CancellationToken ct)
    {
        var result = await tagGenerationService.GenerateForModelAsync(id, ct);
        if (result == null) return NotFound();
        return Ok(result);
    }

    [HttpDelete("{id:guid}/tags/generated")]
    public async Task<IActionResult> ClearGeneratedTags(Guid id, CancellationToken ct)
    {
        var cleared = await tagGenerationService.ClearGeneratedTagsAsync(id, ct);
        if (!cleared) return NotFound();
        return NoContent();
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
                sphereRadius = geometry.SphereRadius,
                sphereCentre = new { geometry.SphereCentre.X, geometry.SphereCentre.Y, geometry.SphereCentre.Z },
                dimensionXMm = geometry.DimensionXMm,
                dimensionYMm = geometry.DimensionYMm,
                dimensionZMm = geometry.DimensionZMm
            });

        var payload = meshTransferService.Encode(geometry);
        return File(payload, MeshTransferService.ContentType);
    }

    /// <summary>
    /// Returns body and support geometry for a pre-supported model in a single binary envelope,
    /// running <see cref="SupportSeparationService.Separate"/> exactly once.
    ///
    /// Envelope format: [bodyLength:uint32 LE][body FMSH bytes][supportLength:uint32 LE][support FMSH bytes].
    ///
    /// Returns 204 No Content when the model is not marked as supported, when no support
    /// components are detected, or when the file cannot be loaded.
    /// </summary>
    [HttpGet("{id:guid}/geometry/split")]
    public async Task<IActionResult> GetSplitGeometry(Guid id)
    {
        var model = await modelService.GetModelAsync(id);
        if (model == null) return NotFound();

        if (model.CalculatedSupported != true)
            return NoContent();

        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath)) return StatusCode(500);

        var fullPath = string.IsNullOrEmpty(model.Directory)
            ? Path.Combine(modelsPath, model.FileName)
            : Path.Combine(modelsPath, model.Directory, model.FileName);

        if (!System.IO.File.Exists(fullPath)) return NotFound();

        var geometry = await loaderService.LoadModelAsync(fullPath, model.FileType);
        if (geometry == null) return NoContent();

        var (bodyTriangles, supports) = supportSeparation.Separate(geometry.Triangles);
        if (supports == null || supports.Count == 0)
            return NoContent();

        var bodyGeometry = new LoadedGeometry
        {
            Triangles = bodyTriangles,
            DimensionXMm = geometry.DimensionXMm,
            DimensionYMm = geometry.DimensionYMm,
            DimensionZMm = geometry.DimensionZMm,
            SphereCentre = geometry.SphereCentre,
            SphereRadius = geometry.SphereRadius,
        };

        var supportGeometry = new LoadedGeometry
        {
            Triangles = supports,
            DimensionXMm = geometry.DimensionXMm,
            DimensionYMm = geometry.DimensionYMm,
            DimensionZMm = geometry.DimensionZMm,
            SphereCentre = geometry.SphereCentre,
            SphereRadius = geometry.SphereRadius,
        };

        var bodyPayload = meshTransferService.Encode(bodyGeometry);
        var supportPayload = meshTransferService.Encode(supportGeometry);

        // Envelope: [bodyLength:uint32][body bytes][supportLength:uint32][support bytes]
        var envelope = new byte[4 + bodyPayload.Length + 4 + supportPayload.Length];
        var span = envelope.AsSpan();
        BinaryPrimitives.WriteUInt32LittleEndian(span[0..4], (uint)bodyPayload.Length);
        bodyPayload.CopyTo(span[4..]);
        int afterBody = 4 + bodyPayload.Length;
        BinaryPrimitives.WriteUInt32LittleEndian(span[afterBody..(afterBody + 4)], (uint)supportPayload.Length);
        supportPayload.CopyTo(span[(afterBody + 4)..]);

        return File(envelope, MeshTransferService.ContentTypeSplit);
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
