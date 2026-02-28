using Microsoft.AspNetCore.Mvc;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class ModelsController(ModelService modelService, IConfiguration config) : ControllerBase
{
    [HttpGet]
    public async Task<IActionResult> GetModels()
    {
        var models = await modelService.GetModelsAsync();
        return Ok(models);
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
    public async Task<IActionResult> GetPreview(Guid id)
    {
        var png = await modelService.GetPreviewImageAsync(id);
        if (png == null) return NotFound();
        return File(png, "image/png");
    }
}
