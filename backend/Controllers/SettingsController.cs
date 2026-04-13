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
    ApplicationLogBuffer applicationLogBuffer) : ControllerBase
{
    [HttpGet("config")]
    public async Task<ActionResult<AppConfigDto>> GetConfig()
    {
        var result = await appConfigService.GetAsync();
        return Ok(result);
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
