using Microsoft.AspNetCore.Mvc;
using findamodel.Models;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/indexer")]
public class IndexerController(IndexerService indexerService) : ControllerBase
{
    /// <summary>
    /// GET /api/indexer
    /// Returns the current queue state: what is running and what is waiting.
    /// </summary>
    [HttpGet]
    public ActionResult<IndexerStatusDto> GetStatus()
    {
        return Ok(indexerService.GetStatus());
    }

    /// <summary>
    /// POST /api/indexer
    /// Enqueues an indexing request. If a request for the same directory already
    /// exists in the queue it is moved to the front and its flags are merged.
    /// </summary>
    [HttpPost]
    public ActionResult<IndexRequestDto> Enqueue([FromBody] EnqueueIndexRequest request)
    {
        if (!string.IsNullOrWhiteSpace(request.DirectoryFilter) && !string.IsNullOrWhiteSpace(request.RelativeModelPath))
            return BadRequest("Specify either directoryFilter or relativeModelPath, not both.");

        var result = indexerService.Enqueue(request.DirectoryFilter, request.RelativeModelPath, request.Flags);
        return Ok(result);
    }
}
