using System.Security.Claims;
using findamodel.Models;
using findamodel.Services;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace findamodel.Controllers;

[ApiController]
[Route("api/printing-lists")]
[Authorize]
public class PrintingListsController(PrintingListService printingListService) : ControllerBase
{
    private Guid CurrentUserId => Guid.Parse(User.FindFirstValue(ClaimTypes.NameIdentifier)!);
    private bool IsAdmin => bool.TryParse(User.FindFirstValue("IsAdmin"), out var v) && v;

    // GET /api/printing-lists
    [HttpGet]
    public async Task<IActionResult> GetLists()
    {
        var lists = await printingListService.GetListsAsync(CurrentUserId, IsAdmin);
        return Ok(lists);
    }

    // GET /api/printing-lists/active
    [HttpGet("active")]
    public async Task<IActionResult> GetActiveList()
    {
        var list = await printingListService.GetActiveListAsync(CurrentUserId);
        if (list == null) return NoContent();
        return Ok(list);
    }

    // GET /api/printing-lists/{id}
    [HttpGet("{id:guid}")]
    public async Task<IActionResult> GetList(Guid id)
    {
        var list = await printingListService.GetListAsync(id, CurrentUserId, IsAdmin);
        if (list == null) return NotFound();
        return Ok(list);
    }

    // POST /api/printing-lists
    [HttpPost]
    public async Task<IActionResult> CreateList([FromBody] CreatePrintingListRequest request)
    {
        if (string.IsNullOrWhiteSpace(request.Name))
            return BadRequest("Name is required.");

        var list = await printingListService.CreateListAsync(CurrentUserId, request.Name.Trim());
        return CreatedAtAction(nameof(GetList), new { id = list.Id }, list);
    }

    // PUT /api/printing-lists/{id}
    [HttpPut("{id:guid}")]
    public async Task<IActionResult> RenameList(Guid id, [FromBody] RenamePrintingListRequest request)
    {
        if (string.IsNullOrWhiteSpace(request.Name))
            return BadRequest("Name is required.");

        var (result, dto) = await printingListService.RenameListAsync(id, CurrentUserId, IsAdmin, request.Name.Trim());
        return result switch
        {
            PrintingListMutateResult.NotFound => NotFound(),
            PrintingListMutateResult.IsDefault => Conflict("The default list cannot be renamed."),
            _ => Ok(dto),
        };
    }

    // DELETE /api/printing-lists/{id}
    [HttpDelete("{id:guid}")]
    public async Task<IActionResult> DeleteList(Guid id)
    {
        var result = await printingListService.DeleteListAsync(id, CurrentUserId, IsAdmin);
        return result switch
        {
            PrintingListMutateResult.NotFound => NotFound(),
            PrintingListMutateResult.IsDefault => Conflict("The default list cannot be deleted."),
            _ => NoContent(),
        };
    }

    // POST /api/printing-lists/{id}/activate
    [HttpPost("{id:guid}/activate")]
    public async Task<IActionResult> ActivateList(Guid id)
    {
        var ok = await printingListService.ActivateListAsync(id, CurrentUserId, IsAdmin);
        if (!ok) return NotFound();
        return NoContent();
    }

    // PUT /api/printing-lists/{id}/items/{modelId}  (id may be "active" or a GUID)
    [HttpPut("{id}/items/{modelId:guid}")]
    public async Task<IActionResult> UpsertItem(string id, Guid modelId, [FromBody] UpsertPrintingListItemRequest request)
    {
        var listId = await printingListService.ResolveListIdAsync(id, CurrentUserId);
        if (listId == null) return NotFound();
        var list = await printingListService.UpsertItemAsync(listId.Value, CurrentUserId, IsAdmin, modelId, request.Quantity);
        if (list == null) return NotFound();
        return Ok(list);
    }

    // DELETE /api/printing-lists/{id}/items/{modelId}  (id may be "active" or a GUID)
    [HttpDelete("{id}/items/{modelId:guid}")]
    public async Task<IActionResult> RemoveItem(string id, Guid modelId)
    {
        var listId = await printingListService.ResolveListIdAsync(id, CurrentUserId);
        if (listId == null) return NotFound();
        var list = await printingListService.UpsertItemAsync(listId.Value, CurrentUserId, IsAdmin, modelId, 0);
        if (list == null) return NotFound();
        return Ok(list);
    }

    // DELETE /api/printing-lists/{id}/items  (id may be "active" or a GUID)
    [HttpDelete("{id}/items")]
    public async Task<IActionResult> ClearItems(string id)
    {
        var listId = await printingListService.ResolveListIdAsync(id, CurrentUserId);
        if (listId == null) return NotFound();
        var list = await printingListService.ClearItemsAsync(listId.Value, CurrentUserId, IsAdmin);
        if (list == null) return NotFound();
        return Ok(list);
    }
}
