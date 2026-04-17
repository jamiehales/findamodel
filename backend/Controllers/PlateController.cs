using findamodel.Models;
using findamodel.Services;
using Microsoft.AspNetCore.Mvc;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class PlateController(
    PlateExportService plateExportService,
    PlateGenerationJobService plateGenerationJobService) : ControllerBase
{
    [HttpPost("generate")]
    public async Task<IActionResult> GeneratePlate(
        [FromBody] GeneratePlateRequest request,
        CancellationToken cancellationToken = default)
    {
        try
        {
            var result = await plateExportService.GeneratePlateAsync(request, cancellationToken: cancellationToken);
            AppendWarningHeaders(result.Warning, result.SkippedModels);
            return File(result.Content, result.ContentType, result.FileName);
        }
        catch (Exception ex)
        {
            return MapGenerationException(ex);
        }
    }

    [HttpPost("jobs")]
    public async Task<IActionResult> CreateGenerationJob(
        [FromBody] GeneratePlateRequest request,
        CancellationToken cancellationToken = default)
    {
        try
        {
            var job = await plateGenerationJobService.CreateJobAsync(request, cancellationToken);
            return AcceptedAtAction(nameof(GetGenerationJobStatus), new { jobId = job.JobId }, job);
        }
        catch (Exception ex)
        {
            return MapGenerationException(ex);
        }
    }

    [HttpGet("jobs/{jobId:guid}")]
    public IActionResult GetGenerationJobStatus(Guid jobId)
    {
        var job = plateGenerationJobService.GetJob(jobId);
        if (job == null) return NotFound();
        return Ok(job);
    }

    [HttpGet("jobs/{jobId:guid}/file")]
    public IActionResult DownloadGenerationJobFile(Guid jobId)
    {
        var file = plateGenerationJobService.GetCompletedJobFile(jobId);
        if (file == null) return NotFound();

        AppendWarningHeaders(file.Value.Warning, file.Value.SkippedModels);
        Response.OnCompleted(() => plateGenerationJobService.RemoveJobAsync(jobId));

        var contentType = Path.GetExtension(file.Value.FileName).ToLowerInvariant() switch
        {
            ".stl" => "model/stl",
            ".glb" => "model/gltf-binary",
            ".zip" => "application/zip",
            _ => "application/vnd.ms-3mf",
        };

        return PhysicalFile(file.Value.Path, contentType, file.Value.FileName);
    }

    private void AppendWarningHeaders(string? warning, IReadOnlyList<string> skippedModels)
    {
        if (string.IsNullOrWhiteSpace(warning) && skippedModels.Count == 0) return;

        if (!string.IsNullOrWhiteSpace(warning))
            Response.Headers.Append("X-Plate-Warning", warning);

        if (skippedModels.Count > 0)
            Response.Headers.Append("X-Plate-Skipped-Models", string.Join(',', skippedModels));
    }

    private IActionResult MapGenerationException(Exception ex) => ex switch
    {
        ArgumentException => BadRequest(ex.Message),
        KeyNotFoundException => NotFound(ex.Message),
        FileNotFoundException => NotFound(ex.Message),
        PlateExportUnprocessableException => UnprocessableEntity(ex.Message),
        _ => StatusCode(500, ex.Message),
    };
}
