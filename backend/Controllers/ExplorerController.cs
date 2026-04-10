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
