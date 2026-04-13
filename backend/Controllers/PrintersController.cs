using findamodel.Models;
using findamodel.Services;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace findamodel.Controllers;

[ApiController]
[Route("api/printers")]
[Authorize]
public class PrintersController(PrinterService printerService) : ControllerBase
{
    // GET /api/printers
    [HttpGet]
    public async Task<IActionResult> GetAll()
    {
        var printers = await printerService.GetAllAsync();
        return Ok(printers);
    }

    // POST /api/printers
    [HttpPost]
    public async Task<IActionResult> Create([FromBody] CreatePrinterConfigRequest request)
    {
        var (dto, error) = await printerService.CreateAsync(request);
        if (error != null) return BadRequest(error);
        return Ok(dto);
    }

    // PUT /api/printers/{id}
    [HttpPut("{id:guid}")]
    public async Task<IActionResult> Update(Guid id, [FromBody] UpdatePrinterConfigRequest request)
    {
        var (dto, error) = await printerService.UpdateAsync(id, request);
        if (dto == null && error == null) return NotFound();
        if (error != null) return BadRequest(error);
        return Ok(dto);
    }

    // DELETE /api/printers/{id}
    [HttpDelete("{id:guid}")]
    public async Task<IActionResult> Delete(Guid id)
    {
        var (found, error) = await printerService.DeleteAsync(id);
        if (!found) return NotFound();
        if (error != null) return Conflict(error);
        return NoContent();
    }

    // POST /api/printers/{id}/set-default
    [HttpPost("{id:guid}/set-default")]
    public async Task<IActionResult> SetDefault(Guid id)
    {
        var (found, error) = await printerService.SetDefaultAsync(id);
        if (!found) return NotFound();
        if (error != null) return BadRequest(error);
        return NoContent();
    }
}
