using System.Globalization;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;

namespace findamodel.Services;

/// <summary>
/// Generates PNG preview images by software-rasterising STL/OBJ geometry.
/// Camera: isometric-style perspective from top-front-left.
/// Lighting: 3-point photography setup (key, fill, rim).
/// </summary>
public class ModelPreviewService(ILogger<ModelPreviewService> logger)
{
    private const int RenderWidth = 512;
    private const int RenderHeight = 512;

    public async Task<byte[]?> GeneratePreviewAsync(string filePath, string fileType)
    {
        try
        {
            List<Triangle3D>? triangles = fileType.ToLowerInvariant() switch
            {
                "stl" => await ParseStlAsync(filePath),
                "obj" => await ParseObjAsync(filePath),
                _ => null
            };

            if (triangles == null || triangles.Count == 0)
            {
                logger.LogWarning("No geometry found in {FilePath}", filePath);
                return null;
            }

            // Models are stored in Z-up (common in CAD / 3-D printing tools).
            // Convert to Y-up so the renderer's worldUp = (0,1,0) is correct.
            triangles = ConvertZUpToYUp(triangles);

            logger.LogInformation("Rendering preview for {FilePath} ({Count} triangles)", filePath, triangles.Count);
            return await Task.Run(() => MeshRenderer.Render(triangles, RenderWidth, RenderHeight));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to generate preview for {FilePath}", filePath);
            return null;
        }
    }

    // ── STL ───────────────────────────────────────────────────────────────────

    private static async Task<List<Triangle3D>?> ParseStlAsync(string path)
    {
        await using var stream = File.OpenRead(path);

        // Read 80-byte header
        var header = new byte[80];
        int read = 0;
        while (read < 80)
        {
            int n = await stream.ReadAsync(header.AsMemory(read, 80 - read));
            if (n == 0) return null;
            read += n;
        }

        // Read triangle count (bytes 80-83)
        var countBuf = new byte[4];
        await stream.ReadExactlyAsync(countBuf);
        uint triCount = BitConverter.ToUInt32(countBuf);

        // Distinguish binary from ASCII by exact file-size match.
        // Binary: exactly 84 + triCount*50 bytes.
        long expectedBinarySize = 84L + (long)triCount * 50;
        if (stream.Length == expectedBinarySize && triCount > 0)
            return await ParseBinaryStlAsync(stream, triCount);

        // Fall back to ASCII (stream already rewound below)
        stream.Position = 0;
        return await ParseAsciiStlAsync(stream);
    }

    private static async Task<List<Triangle3D>> ParseBinaryStlAsync(Stream stream, uint triCount)
    {
        var triangles = new List<Triangle3D>((int)Math.Min(triCount, 5_000_000u));
        var buf = new byte[50];

        for (uint i = 0; i < triCount; i++)
        {
            await stream.ReadExactlyAsync(buf);
            var normal = new Vec3(
                BitConverter.ToSingle(buf, 0),
                BitConverter.ToSingle(buf, 4),
                BitConverter.ToSingle(buf, 8));
            var v0 = new Vec3(
                BitConverter.ToSingle(buf, 12),
                BitConverter.ToSingle(buf, 16),
                BitConverter.ToSingle(buf, 20));
            var v1 = new Vec3(
                BitConverter.ToSingle(buf, 24),
                BitConverter.ToSingle(buf, 28),
                BitConverter.ToSingle(buf, 32));
            var v2 = new Vec3(
                BitConverter.ToSingle(buf, 36),
                BitConverter.ToSingle(buf, 40),
                BitConverter.ToSingle(buf, 44));
            // Skip 2-byte attribute word (already consumed in buf[48-49])
            triangles.Add(new Triangle3D(v0, v1, v2, normal));
        }

        return triangles;
    }

    private static async Task<List<Triangle3D>> ParseAsciiStlAsync(Stream stream)
    {
        var triangles = new List<Triangle3D>();
        using var reader = new StreamReader(stream, leaveOpen: true);

        Vec3 normal = default;
        var verts = new Vec3[3];
        int vertIdx = 0;

        string? line;
        while ((line = await reader.ReadLineAsync()) != null)
        {
            var span = line.AsSpan().TrimStart();
            if (span.StartsWith("facet normal", StringComparison.OrdinalIgnoreCase))
            {
                var parts = line.Trim().Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length >= 5)
                    normal = new Vec3(
                        float.Parse(parts[2], CultureInfo.InvariantCulture),
                        float.Parse(parts[3], CultureInfo.InvariantCulture),
                        float.Parse(parts[4], CultureInfo.InvariantCulture));
                vertIdx = 0;
            }
            else if (span.StartsWith("vertex ", StringComparison.OrdinalIgnoreCase) && vertIdx < 3)
            {
                var parts = line.Trim().Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length >= 4)
                    verts[vertIdx++] = new Vec3(
                        float.Parse(parts[1], CultureInfo.InvariantCulture),
                        float.Parse(parts[2], CultureInfo.InvariantCulture),
                        float.Parse(parts[3], CultureInfo.InvariantCulture));
            }
            else if (span.StartsWith("endfacet", StringComparison.OrdinalIgnoreCase) && vertIdx == 3)
            {
                triangles.Add(new Triangle3D(verts[0], verts[1], verts[2], normal));
            }
        }

        return triangles;
    }

    // ── OBJ ───────────────────────────────────────────────────────────────────

    private static async Task<List<Triangle3D>?> ParseObjAsync(string path)
    {
        var vertices = new List<Vec3>();
        var normals = new List<Vec3>();
        var triangles = new List<Triangle3D>();

        await foreach (var rawLine in File.ReadLinesAsync(path))
        {
            var line = rawLine.Trim();
            if (line.Length == 0 || line[0] == '#') continue;

            if (line.StartsWith("v ", StringComparison.Ordinal))
            {
                var p = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (p.Length >= 4)
                    vertices.Add(new Vec3(
                        float.Parse(p[1], CultureInfo.InvariantCulture),
                        float.Parse(p[2], CultureInfo.InvariantCulture),
                        float.Parse(p[3], CultureInfo.InvariantCulture)));
            }
            else if (line.StartsWith("vn ", StringComparison.Ordinal))
            {
                var p = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (p.Length >= 4)
                    normals.Add(new Vec3(
                        float.Parse(p[1], CultureInfo.InvariantCulture),
                        float.Parse(p[2], CultureInfo.InvariantCulture),
                        float.Parse(p[3], CultureInfo.InvariantCulture)));
            }
            else if (line.StartsWith("f ", StringComparison.Ordinal))
            {
                var p = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (p.Length < 4) continue;

                // Parse all face vertices then fan-triangulate
                var fv = new List<(int vi, int ni)>(p.Length - 1);
                for (int i = 1; i < p.Length; i++)
                    fv.Add(ParseFaceVertex(p[i], vertices.Count, normals.Count));

                for (int i = 1; i < fv.Count - 1; i++)
                {
                    var (vi0, ni0) = fv[0];
                    var (vi1, ni1) = fv[i];
                    var (vi2, ni2) = fv[i + 1];

                    if ((uint)vi0 >= (uint)vertices.Count ||
                        (uint)vi1 >= (uint)vertices.Count ||
                        (uint)vi2 >= (uint)vertices.Count) continue;

                    var v0 = vertices[vi0];
                    var v1 = vertices[vi1];
                    var v2 = vertices[vi2];

                    Vec3 n;
                    if (ni0 >= 0 && ni0 < normals.Count &&
                        ni1 >= 0 && ni1 < normals.Count &&
                        ni2 >= 0 && ni2 < normals.Count)
                    {
                        n = ((normals[ni0] + normals[ni1] + normals[ni2]) * (1f / 3f)).Normalized;
                    }
                    else
                    {
                        n = (v1 - v0).Cross(v2 - v0).Normalized;
                    }

                    triangles.Add(new Triangle3D(v0, v1, v2, n));
                }
            }
        }

        return triangles;
    }

    // Convert Z-up (CAD/3-D printing convention) to Y-up by rotating −90° around X:
    //   x' = x,  y' = z,  z' = −y
    private static List<Triangle3D> ConvertZUpToYUp(List<Triangle3D> triangles)
    {
        var result = new List<Triangle3D>(triangles.Count);
        foreach (var tri in triangles)
            result.Add(new Triangle3D(ZUpToYUp(tri.V0), ZUpToYUp(tri.V1), ZUpToYUp(tri.V2), ZUpToYUp(tri.Normal)));
        return result;
    }

    // Transform from Z-up to Y-up: applies rotateX(π/2) then rotateZ(π)
    // rotateX(π/2): [x, y, z] -> [x, -z, y]
    // rotateZ(π): [x, y, z] -> [-x, -y, z]
    // Combined: [x, y, z] -> [-x, z, y]
    private static Vec3 ZUpToYUp(Vec3 v) => new(-v.X, v.Z, v.Y);

    private static (int vi, int ni) ParseFaceVertex(string token, int vCount, int nCount)
    {
        int slash = token.IndexOf('/');
        if (slash < 0)
        {
            int v = int.Parse(token, CultureInfo.InvariantCulture);
            return (v > 0 ? v - 1 : vCount + v, -1);
        }

        var parts = token.Split('/');
        int vi = int.Parse(parts[0], CultureInfo.InvariantCulture);
        vi = vi > 0 ? vi - 1 : vCount + vi;

        int ni = -1;
        if (parts.Length >= 3 && parts[2].Length > 0)
        {
            int n = int.Parse(parts[2], CultureInfo.InvariantCulture);
            ni = n > 0 ? n - 1 : nCount + n;
        }

        return (vi, ni);
    }
}

// ── Types ─────────────────────────────────────────────────────────────────────

internal readonly record struct Vec3(float X, float Y, float Z)
{
    public static readonly Vec3 Up = new(0, 1, 0);

    public static Vec3 operator +(Vec3 a, Vec3 b) => new(a.X + b.X, a.Y + b.Y, a.Z + b.Z);
    public static Vec3 operator -(Vec3 a, Vec3 b) => new(a.X - b.X, a.Y - b.Y, a.Z - b.Z);
    public static Vec3 operator *(Vec3 v, float s) => new(v.X * s, v.Y * s, v.Z * s);
    public static Vec3 operator *(float s, Vec3 v) => v * s;
    public static Vec3 operator -(Vec3 v) => new(-v.X, -v.Y, -v.Z);

    public float Dot(Vec3 b) => X * b.X + Y * b.Y + Z * b.Z;
    public Vec3 Cross(Vec3 b) => new(Y * b.Z - Z * b.Y, Z * b.X - X * b.Z, X * b.Y - Y * b.X);

    public float LengthSq => X * X + Y * Y + Z * Z;
    public float Length => MathF.Sqrt(LengthSq);
    public Vec3 Normalized
    {
        get
        {
            float len = Length;
            return len < 1e-8f ? Up : this * (1f / len);
        }
    }
}

internal readonly record struct Triangle3D(Vec3 V0, Vec3 V1, Vec3 V2, Vec3 Normal);

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
