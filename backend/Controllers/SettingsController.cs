using Microsoft.AspNetCore.Mvc;
using findamodel.Models;
using findamodel.Services;
using Serilog.Events;

namespace findamodel.Controllers;

[ApiController]
[Route("api/settings")]
public class SettingsController(
    MetadataDictionaryService metadataDictionaryService,
    AppConfigService appConfigService,
    InstanceStatsService instanceStatsService,
    ApplicationLogBuffer applicationLogBuffer,
    AutoSupportSettingsPreviewService autoSupportSettingsPreviewService) : ControllerBase
{
    [HttpGet("config")]
    public async Task<ActionResult<AppConfigDto>> GetConfig()
    {
        var result = await appConfigService.GetAsync();
        return Ok(result);
    }

    [HttpGet("setup-status")]
    public async Task<ActionResult<SetupStatusDto>> GetSetupStatus()
    {
        var status = await appConfigService.GetSetupStatusAsync();
        return Ok(status);
    }

    [HttpGet("setup-defaults")]
    public async Task<ActionResult<InitialSetupDefaultsDto>> GetSetupDefaults()
    {
        var result = await appConfigService.GetInitialSetupDefaultsAsync();
        return Ok(result);
    }

    [HttpPost("setup")]
    public async Task<ActionResult<AppConfigDto>> CompleteSetup([FromBody] InitialSetupRequest request)
    {
        try
        {
            var result = await appConfigService.CompleteInitialSetupAsync(request);
            return Ok(result);
        }
        catch (ArgumentException ex)
        {
            return BadRequest(ex.Message);
        }
    }

    [HttpPut("config")]
    public async Task<ActionResult<AppConfigDto>> UpdateConfig([FromBody] UpdateAppConfigRequest request)
    {
        try
        {
            var result = await appConfigService.UpdateAsync(request);
            return Ok(result);
        }
        catch (ArgumentException ex)
        {
            return BadRequest(ex.Message);
        }
    }

    [HttpPost("auto-support-preview")]
    public async Task<ActionResult<AutoSupportSettingsPreviewDto>> GenerateAutoSupportPreview(
        [FromBody] AutoSupportSettingsPreviewRequest request,
        CancellationToken ct)
    {
        try
        {
            var result = await autoSupportSettingsPreviewService.GeneratePreviewAsync(request, ct);
            return Ok(result);
        }
        catch (ArgumentException ex)
        {
            return BadRequest(ex.Message);
        }
    }

    [HttpGet("auto-support-preview/{previewId:guid}/geometry/{scenarioId}")]
    public IActionResult GetAutoSupportPreviewGeometry(Guid previewId, string scenarioId)
    {
        if (string.IsNullOrWhiteSpace(scenarioId))
            return BadRequest("Scenario id is required.");

        var envelope = autoSupportSettingsPreviewService.GetScenarioEnvelope(previewId, scenarioId);
        if (envelope == null)
            return NotFound();

        Response.Headers.CacheControl = "no-store";
        return File(envelope, MeshTransferService.ContentTypeSplit);
    }

    [HttpGet("metadata-dictionary")]
    public async Task<ActionResult<MetadataDictionaryOverviewDto>> GetMetadataDictionary()
    {
        var result = await metadataDictionaryService.GetOverviewAsync();
        return Ok(result);
    }

    [HttpGet("stats")]
    public async Task<ActionResult<InstanceStatsDto>> GetStats()
    {
        var result = await instanceStatsService.GetAsync();
        return Ok(result);
    }

    [HttpPost("metadata-dictionary")]
    public async Task<ActionResult<MetadataDictionaryValueDto>> CreateMetadataDictionaryValue(
        [FromBody] CreateMetadataDictionaryValueRequest request)
    {
        try
        {
            var result = await metadataDictionaryService.CreateAsync(request.Field, request.Value);
            return Ok(result);
        }
        catch (ArgumentException ex)
        {
            return BadRequest(ex.Message);
        }
    }

    [HttpPut("metadata-dictionary/{id:guid}")]
    public async Task<ActionResult<MetadataDictionaryValueDto>> UpdateMetadataDictionaryValue(
        [FromRoute] Guid id,
        [FromBody] UpdateMetadataDictionaryValueRequest request)
    {
        try
        {
            var result = await metadataDictionaryService.UpdateAsync(id, request.Value);
            if (result == null) return NotFound();
            return Ok(result);
        }
        catch (ArgumentException ex)
        {
            return BadRequest(ex.Message);
        }
        catch (InvalidOperationException ex)
        {
            return Conflict(ex.Message);
        }
    }

    [HttpDelete("metadata-dictionary/{id:guid}")]
    public async Task<IActionResult> DeleteMetadataDictionaryValue([FromRoute] Guid id)
    {
        var deleted = await metadataDictionaryService.DeleteAsync(id);
        if (!deleted) return NotFound();
        return NoContent();
    }

    [HttpGet("logs")]
    public ActionResult<ApplicationLogsResponseDto> GetLogs(
        [FromQuery] string? channel,
        [FromQuery] string? severity,
        [FromQuery] int limit = 500)
    {
        LogEventLevel? minimumSeverity = null;
        if (!string.IsNullOrWhiteSpace(severity))
        {
            if (!Enum.TryParse<LogEventLevel>(severity, ignoreCase: true, out var parsedSeverity))
            {
                return BadRequest("Invalid severity filter.");
            }

            minimumSeverity = parsedSeverity;
        }

        var entries = applicationLogBuffer
            .Get(channel, minimumSeverity, limit)
            .Select(e => new ApplicationLogEntryDto(
                e.Timestamp,
                e.Severity,
                e.Channel,
                e.Message,
                e.Exception))
            .ToList();

        var availableChannels = applicationLogBuffer.GetAvailableChannels();
        var availableSeverities = Enum.GetNames<LogEventLevel>();

        return Ok(new ApplicationLogsResponseDto(entries, availableChannels, availableSeverities));
    }
}
