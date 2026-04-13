using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
using System.Numerics;

namespace findamodel.Services;

/// <summary>
/// Generates PNG preview images for 3D geometry.
/// Delegates file parsing and coordinate transformation to <see cref="ModelLoaderService"/>.
///
/// Rendering path (in priority order):
///   1. <see cref="GlPreviewContext"/> - OpenGL 3.3 hardware renderer with 4× MSAA.
///      Matches the Three.js viewer camera, lighting, and coordinate system exactly.
///   2. <see cref="MeshRenderer"/> - CPU software rasterizer fallback (used when the
///      GL context is unavailable or when Preview:UseGpu=false in appsettings.json).
/// </summary>
public class ModelPreviewService(
    ModelLoaderService loaderService,
    GlPreviewContext glContext,
    SupportSeparationService supportSeparation,
    IConfiguration configuration,
    ILoggerFactory loggerFactory)
{
    /// <summary>
    /// Increment this when the preview rendering implementation changes so that
    /// existing cached previews are automatically invalidated on the next scan.
    /// </summary>
    public const int CurrentPreviewGenerationVersion = 6;

    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Preview);
    private const int RenderWidth = 512;
    private const int RenderHeight = 512;
    private string? _cacheRendersPath;

    private bool UseGpu => configuration.GetValue("Preview:UseGpu", defaultValue: true);

    // Colours matching ModelViewer.tsx: body = #818cf8 (indigo), supports = #f59e0b (amber)
    private static readonly Vector3 BodyColor = new(0x81 / 255f, 0x8c / 255f, 0xf8 / 255f);
    private static readonly Vector3 SupportColor = new(0xf5 / 255f, 0x9e / 255f, 0x0b / 255f);
    private static readonly Vec3 BodyVec3 = new(BodyColor.X, BodyColor.Y, BodyColor.Z);
    private static readonly Vec3 SupportVec3 = new(SupportColor.X, SupportColor.Y, SupportColor.Z);

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
    public sealed record PreviewGenerationResult(string? RelativePath, bool Generated);

    /// <summary>
    /// Renders a preview when needed and reports whether it was generated in this call.
    /// If a hash-matched cached image already exists on disk, it is reused.
    /// </summary>
    public async Task<PreviewGenerationResult> GeneratePreviewWithStatusAsync(LoadedGeometry geometry, string modelFileHash, string fileType = "")
    {
        if (geometry.Triangles.Count == 0)
        {
            logger.LogWarning("GeneratePreviewAsync called with empty geometry");
            return new PreviewGenerationResult(null, false);
        }

        if (_cacheRendersPath == null)
            return new PreviewGenerationResult(null, false);

        var filename = $"{modelFileHash}.png";
        var fullPath = Path.Combine(_cacheRendersPath, filename);
        if (File.Exists(fullPath))
        {
            logger.LogDebug("Reusing cached preview at {Path}", fullPath);
            return new PreviewGenerationResult(filename, false);
        }

        // Separate body from supports so they can be rendered in distinct colours.
        var (bodyTris, supportTris) = await Task.Run(() => supportSeparation.Separate(geometry.Triangles));

        logger.LogInformation(
            "Rendering preview ({BodyCount} body triangles, {SupportCount} support triangles)",
            bodyTris.Count, supportTris?.Count ?? 0);

        byte[]? png = null;

        // GPU path - try first when enabled and available
        if (UseGpu && glContext.IsAvailable)
        {
            try
            {
                png = await glContext.RenderAsync(bodyTris, RenderWidth, RenderHeight, BodyColor, supportTris, SupportColor);
            }
            catch (Exception ex)
            {
                logger.LogWarning(ex, "GPU render failed; falling back to CPU rasterizer");
            }
        }

        // CPU fallback
        if (png == null)
            png = await Task.Run(() => MeshRenderer.Render(bodyTris, RenderWidth, RenderHeight, BodyVec3, supportTris, SupportVec3));

        if (png == null)
            return new PreviewGenerationResult(null, false);

        File.WriteAllBytes(fullPath, png);
        logger.LogInformation("Saved preview to {Path}", fullPath);
        return new PreviewGenerationResult(filename, true);
    }

    public async Task<string?> GeneratePreviewAsync(LoadedGeometry geometry, string modelFileHash, string fileType = "")
    {
        var result = await GeneratePreviewWithStatusAsync(geometry, modelFileHash, fileType);
        return result.RelativePath;
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
            return await GeneratePreviewAsync(geometry, modelFileHash, fileType);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to generate preview for {FilePath}", filePath);
            return null;
        }
    }
}
