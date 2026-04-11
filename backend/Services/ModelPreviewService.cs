using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace findamodel.Services;

/// <summary>
/// Generates PNG preview images for 3D geometry.
/// Delegates file parsing and coordinate transformation to <see cref="ModelLoaderService"/>.
///
/// Rendering path (in priority order):
///   1. <see cref="GlPreviewContext"/> — OpenGL 3.3 hardware renderer with 4× MSAA.
///      Matches the Three.js viewer camera, lighting, and coordinate system exactly.
///   2. <see cref="MeshRenderer"/> — CPU software rasterizer fallback (used when the
///      GL context is unavailable or when Preview:UseGpu=false in appsettings.json).
/// </summary>
public class ModelPreviewService(
    ModelLoaderService loaderService,
    GlPreviewContext glContext,
    IConfiguration configuration,
    ILoggerFactory loggerFactory)
{
    /// <summary>
    /// Increment this when the preview rendering implementation changes so that
    /// existing cached previews are automatically invalidated on the next scan.
    /// </summary>
    public const int CurrentPreviewGenerationVersion = 3;

    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Preview);
    private const int RenderWidth = 512;
    private const int RenderHeight = 512;
    private string? _cacheRendersPath;

    private bool UseGpu => configuration.GetValue("Preview:UseGpu", defaultValue: true);

    /// <summary>
    /// Sets the directory where preview images should be cached on disk.
    /// </summary>
    public void SetCacheDirectory(string path)
    {
        _cacheRendersPath = path;
    }

    /// <summary>
    /// Renders a preview from pre-loaded geometry and saves to disk if cache path is set.
    /// Returns the relative filename if saved, null otherwise.
    /// </summary>
    public async Task<string?> GeneratePreviewAsync(LoadedGeometry geometry, string modelFileHash)
    {
        if (geometry.Triangles.Count == 0)
        {
            logger.LogWarning("GeneratePreviewAsync called with empty geometry");
            return null;
        }

        logger.LogInformation("Rendering preview ({Count} triangles)", geometry.Triangles.Count);

        byte[]? png = null;

        // GPU path — try first when enabled and available
        if (UseGpu && glContext.IsAvailable)
        {
            try
            {
                png = await glContext.RenderAsync(geometry.Triangles, RenderWidth, RenderHeight);
            }
            catch (Exception ex)
            {
                logger.LogWarning(ex, "GPU render failed; falling back to CPU rasterizer");
            }
        }

        // CPU fallback
        if (png == null)
        {
            png = await Task.Run(() => MeshRenderer.Render(geometry.Triangles, RenderWidth, RenderHeight));
        }

        if (png == null || _cacheRendersPath == null) return null;

        // Save to disk with hash-based filename
        var filename = $"{modelFileHash}.png";
        var fullPath = Path.Combine(_cacheRendersPath, filename);
        File.WriteAllBytes(fullPath, png);
        logger.LogInformation("Saved preview to {Path}", fullPath);
        return filename;
    }

    /// <summary>
    /// Loads the file via ModelLoaderService then renders and saves a preview.
    /// Returns the relative filename if saved, null otherwise.
    /// </summary>
    public async Task<string?> GeneratePreviewAsync(string filePath, string fileType, string modelFileHash)
    {
        try
        {
            var geometry = await loaderService.LoadModelAsync(filePath, fileType);
            if (geometry is null)
            {
                logger.LogWarning("No geometry found in {FilePath}", filePath);
                return null;
            }
            logger.LogInformation("Rendering preview for {FilePath} ({Count} triangles)", filePath, geometry.Triangles.Count);
            return await GeneratePreviewAsync(geometry, modelFileHash);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to generate preview for {FilePath}", filePath);
            return null;
        }
    }
}
