using Microsoft.AspNetCore.Mvc;
using findamodel.Models;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class QueryController(QueryService queryService) : ControllerBase
{
    [HttpGet]
    public async Task<IActionResult> Query([FromQuery] ModelQueryRequest request)
    {
        var result = await queryService.QueryModelsAsync(request);
        return Ok(result);
    }

    [HttpGet("options")]
    public async Task<IActionResult> Options([FromQuery] ModelQueryRequest request)
    {
        var options = await queryService.GetFilterOptionsAsync(request);
        return Ok(options);
    }
}
