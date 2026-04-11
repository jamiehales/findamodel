using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace findamodel.Services;

/// <summary>
/// Generates PNG preview images by software-rasterising STL/OBJ geometry.
/// Delegates file parsing and coordinate transformation to <see cref="ModelLoaderService"/>.
/// Camera: bounding-box framed perspective matching DEFAULT_VIEW_DIRECTION in ModelViewer.tsx.
/// Lighting: matches the Three.js scene in ModelViewer.tsx (ambient + 3 directional lights).
/// </summary>
public class ModelPreviewService(
    ModelLoaderService loaderService,
    ILoggerFactory loggerFactory)
{
    /// <summary>
    /// Increment this when the preview rendering implementation changes so that
    /// existing cached previews are automatically invalidated on the next scan.
    /// </summary>
    public const int CurrentPreviewGenerationVersion = 2;

    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Preview);
    private const int RenderWidth = 512;
    private const int RenderHeight = 512;
    private string? _cacheRendersPath;

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
    public Task<string?> GeneratePreviewAsync(LoadedGeometry geometry, string modelFileHash)
    {
        if (geometry.Triangles.Count == 0)
        {
            logger.LogWarning("GeneratePreviewAsync called with empty geometry");
            return Task.FromResult<string?>(null);
        }

        logger.LogInformation("Rendering preview ({Count} triangles)", geometry.Triangles.Count);
        return Task.Run<string?>(() =>
        {
            var png = MeshRenderer.Render(geometry.Triangles, RenderWidth, RenderHeight);
            if (png == null || _cacheRendersPath == null) return null;

            // Save to disk with hash-based filename
            var filename = $"{modelFileHash}.png";
            var fullPath = Path.Combine(_cacheRendersPath, filename);
            File.WriteAllBytes(fullPath, png);
            logger.LogInformation("Saved preview to {Path}", fullPath);
            return filename;
        });
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

// ── Software rasterizer ───────────────────────────────────────────────────────
//
// Improvements over the baseline:
//   • 2× SSAA  — renders at 2× then box-filters down, giving clean edges
//   • Parallel tile rasterisation — Parallel.For over 64×64 tiles; no locking
//     needed because tiles are non-overlapping.
//   • Pre-process pass — projection + shading computed in parallel before tiles.
//   • Camera matches ModelViewer.tsx exactly (direction, bounding-box framing).
//   • Lighting matches ModelViewer.tsx (ambient 0.4 + 3 directional lights).

internal static class MeshRenderer
{
    private const float FramingPadding = 1.15f;  // matches FRAMING_PADDING in ModelViewer.tsx
    private const int SuperSample = 2;       // 2× SSAA: render at 2× then box-filter down
    private const int TileSize = 64;      // tile width/height for parallel rasterisation

    // Lighting matches ModelViewer.tsx:
    //   ambientLight    intensity={0.4}
    //   directionalLight position={[ 5,  8,  5]} intensity={1.2}
    //   directionalLight position={[-4,  2, -2]} intensity={0.4}
    //   directionalLight position={[ 0, -3, -5]} intensity={0.25}
    private static readonly (Vec3 Dir, float I)[] Lights =
    [
        (new Vec3( 5f,  8f,  5f), 1.20f),
        (new Vec3(-4f,  2f, -2f), 0.40f),
        (new Vec3( 0f, -3f, -5f), 0.25f),
    ];

    // Pre-processed triangle: projected screen coords, flat-shaded colour, screen-space AABB.
    // Area == 0 is the sentinel for "invalid / culled" — skipped in rasterisation.
    private readonly record struct PTri(
        float Sx0, float Sy0, float Sz0,
        float Sx1, float Sy1, float Sz1,
        float Sx2, float Sy2, float Sz2,
        Rgba32 Color,
        int MinX, int MaxX, int MinY, int MaxY,
        float Area, float InvArea);

    public static byte[] Render(List<Triangle3D> triangles, int width, int height)
    {
        int ssW = width * SuperSample;
        int ssH = height * SuperSample;

        // ── Bounding-box framing ─────────────────────────────────────────────
        var (center, halfExtents) = BoundingBox(triangles);

        // Camera direction matches DEFAULT_VIEW_DIRECTION in ModelViewer.tsx
        float fovY = MathF.PI / 4f;
        float aspect = (float)ssW / ssH;
        var camDir = new Vec3(1f, 0.8f, -1f).Normalized;
        float dist = CalculateCameraDistanceForBox(halfExtents, camDir, fovY, aspect);
        var eye = center + camDir * dist;

        var forward = (center - eye).Normalized;
        var worldUp = MathF.Abs(forward.Dot(Vec3.Up)) > 0.99f ? new Vec3(0, 0, 1) : Vec3.Up;
        var right = forward.Cross(worldUp).Normalized;
        var up = right.Cross(forward).Normalized;
        float tanHalfFov = MathF.Tan(fovY / 2f);

        // ── Pre-process triangles in parallel ────────────────────────────────
        // BuildPTri has no shared mutable state — each i writes to ptris[i] only.
        var ptris = new PTri[triangles.Count];
        Parallel.For(0, triangles.Count, i =>
            ptris[i] = BuildPTri(triangles[i], eye, forward, right, up, tanHalfFov, aspect, ssW, ssH));

        // ── Parallel tile rasterisation ──────────────────────────────────────
        // Each tile owns its own pixel/zbuf region — no synchronisation needed.
        var ssPixels = new Rgba32[ssW * ssH];
        var zbuf = new float[ssW * ssH];
        Array.Fill(ssPixels, new Rgba32(15, 23, 42));  // #0f172a
        Array.Fill(zbuf, float.MaxValue);

        int tilesX = (ssW + TileSize - 1) / TileSize;
        int tilesY = (ssH + TileSize - 1) / TileSize;
        Parallel.For(0, tilesX * tilesY, tileIdx =>
        {
            int tx = tileIdx % tilesX;
            int ty = tileIdx / tilesX;
            int x0 = tx * TileSize, x1 = Math.Min(x0 + TileSize, ssW) - 1;
            int y0 = ty * TileSize, y1 = Math.Min(y0 + TileSize, ssH) - 1;
            RasterizeTile(ptris, ssPixels, zbuf, ssW, x0, y0, x1, y1);
        });

        // ── 2×2 box-filter downsample + PNG encode ───────────────────────────
        return DownsampleAndEncode(ssPixels, ssW, ssH, width, height);
    }

    private static PTri BuildPTri(
        in Triangle3D tri,
        Vec3 eye, Vec3 forward, Vec3 right, Vec3 up,
        float tanHalfFov, float aspect, int width, int height)
    {
        var n = (tri.V1 - tri.V0).Cross(tri.V2 - tri.V0);
        if (n.LengthSq < 1e-12f) return default;        // degenerate
        n = n.Normalized;
        if (n.Dot(eye - tri.V0) < 0f) return default;  // backface

        var (sx0, sy0, sz0) = Project(tri.V0, eye, forward, right, up, tanHalfFov, aspect, width, height);
        var (sx1, sy1, sz1) = Project(tri.V1, eye, forward, right, up, tanHalfFov, aspect, width, height);
        var (sx2, sy2, sz2) = Project(tri.V2, eye, forward, right, up, tanHalfFov, aspect, width, height);
        if (sz0 <= 0f || sz1 <= 0f || sz2 <= 0f) return default;

        int minX = (int)MathF.Max(0, MathF.Min(sx0, MathF.Min(sx1, sx2)));
        int maxX = (int)MathF.Min(width - 1, MathF.Ceiling(MathF.Max(sx0, MathF.Max(sx1, sx2))));
        int minY = (int)MathF.Max(0, MathF.Min(sy0, MathF.Min(sy1, sy2)));
        int maxY = (int)MathF.Min(height - 1, MathF.Ceiling(MathF.Max(sy0, MathF.Max(sy1, sy2))));
        if (minX > maxX || minY > maxY) return default;

        float area = EdgeFunc(sx0, sy0, sx1, sy1, sx2, sy2);
        if (MathF.Abs(area) < 0.5f) return default;

        var centroid = new Vec3(
            (tri.V0.X + tri.V1.X + tri.V2.X) * (1f / 3f),
            (tri.V0.Y + tri.V1.Y + tri.V2.Y) * (1f / 3f),
            (tri.V0.Z + tri.V1.Z + tri.V2.Z) * (1f / 3f));
        var (fr, fg, fb) = Shade(n, centroid, eye);
        var color = new Rgba32(
            (byte)Math.Clamp((int)(fr * 255f + 0.5f), 0, 255),
            (byte)Math.Clamp((int)(fg * 255f + 0.5f), 0, 255),
            (byte)Math.Clamp((int)(fb * 255f + 0.5f), 0, 255),
            255);

        return new PTri(sx0, sy0, sz0, sx1, sy1, sz1, sx2, sy2, sz2,
                        color, minX, maxX, minY, maxY, area, 1f / area);
    }

    private static void RasterizeTile(
        PTri[] ptris, Rgba32[] pixels, float[] zbuf, int stride,
        int x0, int y0, int x1, int y1)
    {
        foreach (var p in ptris)
        {
            if (p.Area == 0f) continue;  // invalid/culled sentinel
            if (p.MaxX < x0 || p.MinX > x1 || p.MaxY < y0 || p.MinY > y1) continue;

            int cx0 = Math.Max(p.MinX, x0), cx1 = Math.Min(p.MaxX, x1);
            int cy0 = Math.Max(p.MinY, y0), cy1 = Math.Min(p.MaxY, y1);

            for (int y = cy0; y <= cy1; y++)
                for (int x = cx0; x <= cx1; x++)
                {
                    float px = x + 0.5f, py = y + 0.5f;
                    float e0 = EdgeFunc(p.Sx1, p.Sy1, p.Sx2, p.Sy2, px, py);
                    float e1 = EdgeFunc(p.Sx2, p.Sy2, p.Sx0, p.Sy0, px, py);
                    float e2 = EdgeFunc(p.Sx0, p.Sy0, p.Sx1, p.Sy1, px, py);

                    if (p.Area > 0f ? (e0 < 0f || e1 < 0f || e2 < 0f)
                                    : (e0 > 0f || e1 > 0f || e2 > 0f)) continue;

                    float b0 = e0 * p.InvArea, b1 = e1 * p.InvArea;
                    float z = b0 * p.Sz0 + b1 * p.Sz1 + (1f - b0 - b1) * p.Sz2;

                    int idx = y * stride + x;
                    if (z >= zbuf[idx]) continue;
                    zbuf[idx] = z;
                    pixels[idx] = p.Color;
                }
        }
    }

    private static byte[] DownsampleAndEncode(Rgba32[] ssPixels, int ssW, int ssH, int w, int h)
    {
        using var image = new Image<Rgba32>(w, h);
        image.ProcessPixelRows(accessor =>
        {
            for (int y = 0; y < h; y++)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < w; x++)
                {
                    int r = 0, g = 0, b = 0;
                    for (int dy = 0; dy < SuperSample; dy++)
                        for (int dx = 0; dx < SuperSample; dx++)
                        {
                            var c = ssPixels[(y * SuperSample + dy) * ssW + x * SuperSample + dx];
                            r += c.R; g += c.G; b += c.B;
                        }
                    int n = SuperSample * SuperSample;
                    row[x] = new Rgba32((byte)(r / n), (byte)(g / n), (byte)(b / n), 255);
                }
            }
        });
        using var ms = new MemoryStream();
        image.SaveAsPng(ms);
        return ms.ToArray();
    }

    // Ports calculateCameraDistanceForBox from ModelViewer.tsx.
    // direction = FROM target TO camera (same as DEFAULT_VIEW_DIRECTION).
    private static float CalculateCameraDistanceForBox(
        Vec3 halfExtents, Vec3 direction, float fovY, float aspect)
    {
        float halfVertFov = fovY / 2f;
        float halfHorzFov = MathF.Atan(MathF.Tan(halfVertFov) * aspect);
        float tanHalfVert = MathF.Tan(halfVertFov);
        float tanHalfHorz = MathF.Tan(halfHorzFov);

        var toCamera = direction.Normalized;
        var worldUp = MathF.Abs(toCamera.Dot(Vec3.Up)) > 0.999f ? new Vec3(0, 0, 1) : Vec3.Up;
        var right = worldUp.Cross(toCamera).Normalized;
        var up = toCamera.Cross(right).Normalized;

        float reqDist = 0f;
        for (int sx = -1; sx <= 1; sx += 2)
            for (int sy = -1; sy <= 1; sy += 2)
                for (int sz = -1; sz <= 1; sz += 2)
                {
                    var corner = new Vec3(sx * halfExtents.X, sy * halfExtents.Y, sz * halfExtents.Z);
                    float cx = MathF.Abs(corner.Dot(right));
                    float cy = MathF.Abs(corner.Dot(up));
                    float cz = corner.Dot(toCamera);
                    reqDist = MathF.Max(reqDist, MathF.Max(cz + cx / tanHalfHorz, cz + cy / tanHalfVert));
                }
        return MathF.Max(reqDist, 0.001f) * FramingPadding;
    }

    private static (float sx, float sy, float sz) Project(
        Vec3 world, Vec3 eye, Vec3 forward, Vec3 right, Vec3 up,
        float tanHalfFov, float aspect, int width, int height)
    {
        var rel = world - eye;
        float vz = rel.Dot(forward);
        if (vz <= 0.001f) return (0f, 0f, -1f);

        float vx = rel.Dot(right);
        float vy = rel.Dot(up);

        float ndcX = vx / (vz * tanHalfFov * aspect);
        float ndcY = vy / (vz * tanHalfFov);

        float sx = (ndcX + 1f) * 0.5f * width;
        float sy = (1f - ndcY) * 0.5f * height;  // flip Y (screen-space Y is down)
        return (sx, sy, vz);
    }

    private static float EdgeFunc(float ax, float ay, float bx, float by, float px, float py)
        => (px - ax) * (by - ay) - (py - ay) * (bx - ax);

    private static (float r, float g, float b) Shade(Vec3 normal, Vec3 position, Vec3 eye)
    {
        // Off-white neutral plastic — approximate of meshStandardMaterial roughness=0.55 metalness=0.15
        const float mr = 0.86f, mg = 0.86f, mb = 0.89f;
        const float ka = 0.40f;  // ambient — matches Three.js ambientLight intensity={0.4}
        const float kd = 0.70f;  // diffuse
        const float ks = 0.15f;  // specular (low: high roughness in frontend material)
        const float shininess = 20f;

        float tr = ka * mr, tg = ka * mg, tb = ka * mb;
        var viewDir = (eye - position).Normalized;

        foreach (var (dir, intensity) in Lights)
        {
            var lightDir = dir.Normalized;
            float diff = MathF.Max(0f, normal.Dot(lightDir));
            if (diff <= 0f) continue;

            tr += kd * diff * intensity * mr;
            tg += kd * diff * intensity * mg;
            tb += kd * diff * intensity * mb;

            var refl = (normal * (2f * normal.Dot(lightDir)) - lightDir).Normalized;
            float spec = MathF.Pow(MathF.Max(0f, refl.Dot(viewDir)), shininess);
            if (spec > 0f)
            {
                tr += ks * spec * intensity;
                tg += ks * spec * intensity;
                tb += ks * spec * intensity;
            }
        }

        return (tr, tg, tb);
    }

    private static (Vec3 center, Vec3 halfExtents) BoundingBox(List<Triangle3D> tris)
    {
        float minX = float.MaxValue, minY = float.MaxValue, minZ = float.MaxValue;
        float maxX = float.MinValue, maxY = float.MinValue, maxZ = float.MinValue;

        foreach (var t in tris)
        {
            Expand(t.V0); Expand(t.V1); Expand(t.V2);
        }

        var center = new Vec3((minX + maxX) * 0.5f, (minY + maxY) * 0.5f, (minZ + maxZ) * 0.5f);
        var halfExtents = new Vec3((maxX - minX) * 0.5f, (maxY - minY) * 0.5f, (maxZ - minZ) * 0.5f);
        return (center, halfExtents);

        void Expand(Vec3 v)
        {
            if (v.X < minX) minX = v.X; if (v.X > maxX) maxX = v.X;
            if (v.Y < minY) minY = v.Y; if (v.Y > maxY) maxY = v.Y;
            if (v.Z < minZ) minZ = v.Z; if (v.Z > maxZ) maxZ = v.Z;
        }
    }
}
