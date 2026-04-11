using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace findamodel.Services;

/// <summary>
/// Generates PNG preview images by software-rasterising STL/OBJ geometry.
/// Delegates file parsing and coordinate transformation to <see cref="ModelLoaderService"/>.
/// Camera: isometric-style perspective from top-front-left.
/// Lighting: 3-point photography setup (key, fill, rim).
/// </summary>
public class ModelPreviewService(
    ModelLoaderService loaderService,
    ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Preview);
    private const int RenderWidth  = 512;
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

internal static class MeshRenderer
{
    // 3-point photography lighting (directions are in world space, unnormalised here)
    // Key: upper-right-front  — warm white, high intensity, main illumination
    // Fill: upper-left-front  — cool white, medium intensity, reduces shadow harshness
    // Rim: back-upper          — neutral, low-medium intensity, separates model from bg
    private static readonly (Vec3 Dir, float I, float R, float G, float B)[] Lights =
    [
        (new Vec3( 1.2f,  2.0f,  1.5f), 0.85f, 1.00f, 0.97f, 0.92f),  // key
        (new Vec3(-1.5f,  1.0f,  0.8f), 0.40f, 0.88f, 0.92f, 1.00f),  // fill
        (new Vec3(-0.3f,  1.2f, -1.8f), 0.30f, 1.00f, 1.00f, 1.00f),  // rim
    ];

    public static byte[] Render(List<Triangle3D> triangles, int width, int height)
    {
        // ── Bounding sphere ──────────────────────────────────────────────────
        var (center, radius) = BoundingSphere(triangles);
        if (radius < 1e-6f) radius = 1f;

        // ── Camera: top-front-left perspective ───────────────────────────────
        // Camera sits above, in front, and to the left.  The direction vector
        // (-1, 1.5, 1) gives roughly 35° elevation and 45° azimuth left of front.
        float fovY = MathF.PI / 4f;  // 45°
        var camDir = new Vec3(-1f, 1.5f, 1f).Normalized;
        float dist = (radius / MathF.Sin(fovY / 2f)) * 1.2f;  // 20% padding
        var eye = center + camDir * dist;

        // ── View basis ───────────────────────────────────────────────────────
        var forward = (center - eye).Normalized;
        var worldUp = MathF.Abs(forward.Dot(Vec3.Up)) > 0.99f
            ? new Vec3(0, 0, 1) : Vec3.Up;
        var right = forward.Cross(worldUp).Normalized;
        var up = right.Cross(forward).Normalized;

        float tanHalfFov = MathF.Tan(fovY / 2f);
        float aspect = (float)width / height;

        // ── Framebuffer & z-buffer ───────────────────────────────────────────
        var pixels = new Rgba32[width * height];
        var zbuf = new float[width * height];
        var bg = new Rgba32(15, 23, 42);  // #0f172a — matches app dark background
        Array.Fill(pixels, bg);
        Array.Fill(zbuf, float.MaxValue);

        // ── Rasterise ────────────────────────────────────────────────────────
        foreach (var tri in triangles)
            RasterizeTri(in tri, eye, forward, right, up, tanHalfFov, aspect, width, height, pixels, zbuf);

        // ── Encode PNG ───────────────────────────────────────────────────────
        using var image = new Image<Rgba32>(width, height);
        image.ProcessPixelRows(accessor =>
        {
            for (int y = 0; y < height; y++)
            {
                var row = accessor.GetRowSpan(y);
                for (int x = 0; x < width; x++)
                    row[x] = pixels[y * width + x];
            }
        });

        using var ms = new MemoryStream();
        image.SaveAsPng(ms);
        return ms.ToArray();
    }

    private static void RasterizeTri(
        in Triangle3D tri,
        Vec3 eye, Vec3 forward, Vec3 right, Vec3 up,
        float tanHalfFov, float aspect, int width, int height,
        Rgba32[] pixels, float[] zbuf)
    {
        // Compute face normal from geometry (more reliable than stored normal)
        var geomNormal = (tri.V1 - tri.V0).Cross(tri.V2 - tri.V0);
        if (geomNormal.LengthSq < 1e-12f) return;  // degenerate triangle
        geomNormal = geomNormal.Normalized;

        // Backface culling — skip faces pointing away from camera
        if (geomNormal.Dot(eye - tri.V0) < 0f) return;

        // Project all three vertices
        var (sx0, sy0, sz0) = Project(tri.V0, eye, forward, right, up, tanHalfFov, aspect, width, height);
        var (sx1, sy1, sz1) = Project(tri.V1, eye, forward, right, up, tanHalfFov, aspect, width, height);
        var (sx2, sy2, sz2) = Project(tri.V2, eye, forward, right, up, tanHalfFov, aspect, width, height);

        // Clip: any vertex behind camera → skip
        if (sz0 <= 0f || sz1 <= 0f || sz2 <= 0f) return;

        // Screen-space bounding box clamped to viewport
        int minX = (int)MathF.Max(0, MathF.Min(sx0, MathF.Min(sx1, sx2)));
        int maxX = (int)MathF.Min(width - 1, MathF.Ceiling(MathF.Max(sx0, MathF.Max(sx1, sx2))));
        int minY = (int)MathF.Max(0, MathF.Min(sy0, MathF.Min(sy1, sy2)));
        int maxY = (int)MathF.Min(height - 1, MathF.Ceiling(MathF.Max(sy0, MathF.Max(sy1, sy2))));
        if (minX > maxX || minY > maxY) return;

        // Signed area of screen-space triangle
        float area = EdgeFunc(sx0, sy0, sx1, sy1, sx2, sy2);
        if (MathF.Abs(area) < 0.5f) return;
        float invArea = 1f / area;

        // Flat-shade: compute Phong lighting once at the face centroid
        var centroid = new Vec3(
            (tri.V0.X + tri.V1.X + tri.V2.X) * (1f / 3f),
            (tri.V0.Y + tri.V1.Y + tri.V2.Y) * (1f / 3f),
            (tri.V0.Z + tri.V1.Z + tri.V2.Z) * (1f / 3f));
        var (fr, fg, fb) = Shade(geomNormal, centroid, eye);
        var pixel = new Rgba32(
            (byte)Math.Clamp((int)(fr * 255f + 0.5f), 0, 255),
            (byte)Math.Clamp((int)(fg * 255f + 0.5f), 0, 255),
            (byte)Math.Clamp((int)(fb * 255f + 0.5f), 0, 255),
            255);

        // Per-pixel edge-function rasterisation with depth test
        for (int y = minY; y <= maxY; y++)
        {
            for (int x = minX; x <= maxX; x++)
            {
                float cx = x + 0.5f, cy = y + 0.5f;
                float e0 = EdgeFunc(sx1, sy1, sx2, sy2, cx, cy);
                float e1 = EdgeFunc(sx2, sy2, sx0, sy0, cx, cy);
                float e2 = EdgeFunc(sx0, sy0, sx1, sy1, cx, cy);

                if (area > 0f && (e0 < 0f || e1 < 0f || e2 < 0f)) continue;
                if (area < 0f && (e0 > 0f || e1 > 0f || e2 > 0f)) continue;

                // Barycentric interpolation of depth
                float b0 = e0 * invArea, b1 = e1 * invArea;
                float z = b0 * sz0 + b1 * sz1 + (1f - b0 - b1) * sz2;

                int idx = y * width + x;
                if (z >= zbuf[idx]) continue;
                zbuf[idx] = z;
                pixels[idx] = pixel;
            }
        }
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
        // Off-white material with very slight blue tint (neutral plastic look)
        const float mr = 0.86f, mg = 0.86f, mb = 0.89f;
        const float ka = 0.10f;  // ambient
        const float kd = 0.75f;  // diffuse
        const float ks = 0.20f;  // specular
        const float shininess = 20f;

        float tr = ka * mr, tg = ka * mg, tb = ka * mb;
        var viewDir = (eye - position).Normalized;

        foreach (var (dir, intensity, lr, lg, lb) in Lights)
        {
            var lightDir = dir.Normalized;
            float diff = MathF.Max(0f, normal.Dot(lightDir));
            if (diff <= 0f) continue;

            tr += kd * diff * intensity * mr * lr;
            tg += kd * diff * intensity * mg * lg;
            tb += kd * diff * intensity * mb * lb;

            // Phong specular: R = 2*(N·L)*N - L
            var refl = (normal * (2f * normal.Dot(lightDir)) - lightDir).Normalized;
            float spec = MathF.Pow(MathF.Max(0f, refl.Dot(viewDir)), shininess);
            if (spec > 0f)
            {
                tr += ks * spec * intensity * lr;
                tg += ks * spec * intensity * lg;
                tb += ks * spec * intensity * lb;
            }
        }

        return (tr, tg, tb);
    }

    private static (Vec3 center, float radius) BoundingSphere(List<Triangle3D> tris)
    {
        float minX = float.MaxValue, minY = float.MaxValue, minZ = float.MaxValue;
        float maxX = float.MinValue, maxY = float.MinValue, maxZ = float.MinValue;

        foreach (var t in tris)
        {
            Expand(t.V0); Expand(t.V1); Expand(t.V2);
        }

        var center = new Vec3(
            (minX + maxX) * 0.5f,
            (minY + maxY) * 0.5f,
            (minZ + maxZ) * 0.5f);

        float radius = 0f;
        foreach (var t in tris)
        {
            radius = MathF.Max(radius, (t.V0 - center).Length);
            radius = MathF.Max(radius, (t.V1 - center).Length);
            radius = MathF.Max(radius, (t.V2 - center).Length);
        }

        return (center, radius);

        void Expand(Vec3 v)
        {
            if (v.X < minX) minX = v.X; if (v.X > maxX) maxX = v.X;
            if (v.Y < minY) minY = v.Y; if (v.Y > maxY) maxY = v.Y;
            if (v.Z < minZ) minZ = v.Z; if (v.Z > maxZ) maxZ = v.Z;
        }
    }
}
