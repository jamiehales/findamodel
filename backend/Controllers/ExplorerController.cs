using Microsoft.AspNetCore.Mvc;
using findamodel.Models;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/explorer")]
public class ExplorerController(
    ExplorerService explorerService,
    MetadataConfigService metadataConfigService,
    IConfiguration configuration) : ControllerBase
{
    private static readonly HashSet<string> PreviewableFileExtensions =
        new(StringComparer.OrdinalIgnoreCase) { "png", "jpg", "jpeg", "gif", "webp", "txt", "md" };

    /// <summary>
    /// GET /api/explorer?path=some/relative/path
    /// Lists subdirectories and model files at the given path (empty = root).
    /// </summary>
    [HttpGet]
    public async Task<ActionResult<ExplorerResponseDto>> GetDirectory([FromQuery] string path = "")
    {
        try
        {
            var result = await explorerService.GetDirectoryContentsAsync(path);
            return Ok(result);
        }
        catch (DirectoryNotFoundException)
        {
            return NotFound($"Directory not found: {path}");
        }
    }

    /// <summary>
    /// GET /api/explorer/file?path=some/relative/file
    /// Streams an explorer-previewable file (image/text) directly from the models root.
    /// </summary>
    [HttpGet("file")]
    public IActionResult GetFile([FromQuery] string path)
    {
        var modelsRoot = configuration["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsRoot))
            return StatusCode(500, "Models:DirectoryPath is not configured.");

        if (string.IsNullOrWhiteSpace(path))
            return BadRequest("File path is required.");

        var resolvedPath = Path.Combine(modelsRoot, path.Replace('/', Path.DirectorySeparatorChar));
        var fullPath = Path.GetFullPath(resolvedPath);
        var fullRoot = Path.GetFullPath(modelsRoot);

        if (!fullPath.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase))
            return BadRequest("Invalid file path.");

        var ext = Path.GetExtension(fullPath).TrimStart('.').ToLowerInvariant();
        if (!PreviewableFileExtensions.Contains(ext))
            return BadRequest("Unsupported file type.");

        if (!System.IO.File.Exists(fullPath))
            return NotFound();

        var contentType = ext switch
        {
            "png" => "image/png",
            "jpg" or "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "txt" or "md" => "text/plain; charset=utf-8",
            _ => "application/octet-stream",
        };

        return PhysicalFile(fullPath, contentType, enableRangeProcessing: true);
    }

    /// <summary>
    /// GET /api/explorer/config?path=some/relative/path
    /// Returns local and inherited metadata config for a directory.
    /// </summary>
    [HttpGet("config")]
    public async Task<ActionResult<DirectoryConfigDetailDto>> GetConfig([FromQuery] string path = "")
    {
        var result = await metadataConfigService.GetDirectoryConfigDetailAsync(path);
        return Ok(result);
    }

    /// <summary>
    /// PUT /api/explorer/config?path=some/relative/path
    /// Replaces the local metadata config for a directory: writes findamodel.yaml, updates DB,
    /// and re-resolves all descendant DirectoryConfig records.
    /// </summary>
    [HttpPut("config")]
    public async Task<ActionResult<DirectoryConfigDetailDto>> UpdateConfig(
        [FromQuery] string path,
        [FromBody] UpdateDirectoryConfigRequest request)
    {
        var modelsRoot = configuration["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsRoot))
            return StatusCode(500, "Models:DirectoryPath is not configured.");

        try
        {
            var result = await metadataConfigService.UpdateDirectoryConfigAsync(modelsRoot, path, request);
            return Ok(result);
        }
        catch (ConfigValidationException ex)
        {
            return UnprocessableEntity(new ConfigValidationErrorResponse(ex.FieldErrors));
        }
        catch (DirectoryNotFoundException)
        {
            return NotFound($"Directory not found: {path}");
        }
    }
}
