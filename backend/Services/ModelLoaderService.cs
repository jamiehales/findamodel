using System.Globalization;

namespace findamodel.Services;

/// <summary>
/// Loads STL/OBJ model files into a unified <see cref="LoadedGeometry"/> representation.
///
/// Transformations applied (in order):
///   1. Z-up → Y-up: (x,y,z) → (-x,z,y)  via rotateX(π/2) then rotateZ(π)
///   2. Scale to mm: factor = 1.0 (STL/OBJ carry no unit metadata; assumed mm)
///   3. Centre: X/Z centred at origin; Y translated so base face sits at Y = 0
///
/// Thread-safe: stateless; safe for singleton DI registration.
/// </summary>
public class ModelLoaderService(ILoggerFactory loggerFactory)
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Loader);
    /// <summary>
    /// Loads, transforms, centres, and computes geometry metadata for a model file.
    /// Returns null if parsing fails or yields no triangles.
    /// </summary>
    public async Task<LoadedGeometry?> LoadModelAsync(string filePath, string fileType)
    {
        try
        {
            List<Triangle3D>? rawTriangles = fileType.ToLowerInvariant() switch
            {
                "stl" => await ParseStlAsync(filePath),
                "obj" => await ParseObjAsync(filePath),
                _ => null
            };

            if (rawTriangles == null || rawTriangles.Count == 0)
            {
                logger.LogWarning("No geometry found in {FilePath}", filePath);
                return null;
            }

            // ── 1. Z-up → Y-up ───────────────────────────────────────────────
            var yUpTriangles = ApplyZUpToYUp(rawTriangles);

            // ── 2. AABB before centring (finite vertices only) ───────────────
            float minX = float.MaxValue, minY = float.MaxValue, minZ = float.MaxValue;
            float maxX = float.MinValue, maxY = float.MinValue, maxZ = float.MinValue;
            var validTriangles = new List<Triangle3D>(yUpTriangles.Count);
            var discardedTriangleCount = 0;

            foreach (var tri in yUpTriangles)
            {
                if (!IsFinite(tri.V0) || !IsFinite(tri.V1) || !IsFinite(tri.V2))
                {
                    discardedTriangleCount++;
                    continue;
                }

                validTriangles.Add(tri);
                ExpandAabb(tri.V0, ref minX, ref maxX, ref minY, ref maxY, ref minZ, ref maxZ);
                ExpandAabb(tri.V1, ref minX, ref maxX, ref minY, ref maxY, ref minZ, ref maxZ);
                ExpandAabb(tri.V2, ref minX, ref maxX, ref minY, ref maxY, ref minZ, ref maxZ);
            }

            if (validTriangles.Count == 0)
            {
                logger.LogWarning("No finite geometry found in {FilePath}", filePath);
                return null;
            }

            if (discardedTriangleCount > 0)
                logger.LogWarning("Discarded {Count} invalid triangles containing non-finite coordinates in {FilePath}", discardedTriangleCount, filePath);

            float dimX = maxX - minX;
            float dimY = maxY - minY;
            float dimZ = maxZ - minZ;

            if (!IsFinite(dimX) || !IsFinite(dimY) || !IsFinite(dimZ) || dimX < 0f || dimY < 0f || dimZ < 0f)
            {
                logger.LogWarning("Computed invalid model dimensions for {FilePath}: X={DimX}, Y={DimY}, Z={DimZ}", filePath, dimX, dimY, dimZ);
                return null;
            }

            // ── 3. Centre (X/Z midpoint → 0, base Y → 0) ────────────────────
            float offsetX = -(minX + maxX) * 0.5f;
            float offsetY = -minY;
            float offsetZ = -(minZ + maxZ) * 0.5f;

            var centred = new List<Triangle3D>(validTriangles.Count);
            foreach (var tri in validTriangles)
            {
                var v0 = Translate(tri.V0, offsetX, offsetY, offsetZ);
                var v1 = Translate(tri.V1, offsetX, offsetY, offsetZ);
                var v2 = Translate(tri.V2, offsetX, offsetY, offsetZ);
                if (!IsFinite(v0) || !IsFinite(v1) || !IsFinite(v2))
                    continue;

                centred.Add(new Triangle3D(v0, v1, v2, tri.Normal)); // normals are directions - translation doesn't affect them
            }

            if (centred.Count == 0)
            {
                logger.LogWarning("No finite centred geometry found in {FilePath}", filePath);
                return null;
            }

            // ── 4. Bounding sphere of centred model ──────────────────────────
            // Sphere centre = AABB centre of centred model = (0, dimY/2, 0)
            var sphereCentre = new Vec3(0f, dimY * 0.5f, 0f);
            if (!IsFinite(sphereCentre))
            {
                logger.LogWarning("Computed invalid sphere centre for {FilePath}", filePath);
                return null;
            }

            float sphereRadius = 0f;
            var anyValidDistance = false;
            foreach (var tri in centred)
            {
                if (TryDistance(tri.V0, sphereCentre, out var d0))
                {
                    sphereRadius = MathF.Max(sphereRadius, d0);
                    anyValidDistance = true;
                }

                if (TryDistance(tri.V1, sphereCentre, out var d1))
                {
                    sphereRadius = MathF.Max(sphereRadius, d1);
                    anyValidDistance = true;
                }

                if (TryDistance(tri.V2, sphereCentre, out var d2))
                {
                    sphereRadius = MathF.Max(sphereRadius, d2);
                    anyValidDistance = true;
                }
            }

            if (!anyValidDistance || !IsFinite(sphereRadius) || sphereRadius < 1e-6f)
                sphereRadius = 1f;

            logger.LogInformation(
                "Loaded {FilePath}: {Count} triangles, {X:F2}×{Y:F2}×{Z:F2} mm, sphere r={R:F2} mm",
                filePath, centred.Count, dimX, dimY, dimZ, sphereRadius);

            return new LoadedGeometry
            {
                Triangles = centred,
                SphereCentre = sphereCentre,
                SphereRadius = sphereRadius,
                DimensionXMm = dimX,
                DimensionYMm = dimY,
                DimensionZMm = dimZ
            };
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to load model {FilePath}", filePath);
            return null;
        }
    }

    // ── Transforms ───────────────────────────────────────────────────────────

    // Z-up (CAD/slicer) → Y-up (renderer): rotateX(π/2) then rotateZ(π)
    //   rotateX(π/2): (x,y,z) → (x,-z,y)
    //   rotateZ(π):   (x,y,z) → (-x,-y,z)
    //   combined:     (x,y,z) → (-x,z,y)
    private static Vec3 ZUpToYUp(Vec3 v) => new(-v.X, v.Z, v.Y);

    private static List<Triangle3D> ApplyZUpToYUp(List<Triangle3D> triangles)
    {
        var result = new List<Triangle3D>(triangles.Count);
        foreach (var tri in triangles)
            result.Add(new Triangle3D(
                ZUpToYUp(tri.V0), ZUpToYUp(tri.V1), ZUpToYUp(tri.V2),
                ZUpToYUp(tri.Normal)));
        return result;
    }

    private static Vec3 Translate(Vec3 v, float dx, float dy, float dz)
        => new(v.X + dx, v.Y + dy, v.Z + dz);

    private static bool IsFinite(float value) => !float.IsNaN(value) && !float.IsInfinity(value);

    private static bool IsFinite(Vec3 v) => IsFinite(v.X) && IsFinite(v.Y) && IsFinite(v.Z);

    private static bool TryDistance(Vec3 a, Vec3 b, out float distance)
    {
        var dx = (double)a.X - b.X;
        var dy = (double)a.Y - b.Y;
        var dz = (double)a.Z - b.Z;

        var distSq = dx * dx + dy * dy + dz * dz;
        if (double.IsNaN(distSq) || double.IsInfinity(distSq) || distSq < 0)
        {
            distance = 0f;
            return false;
        }

        var dist = Math.Sqrt(distSq);
        if (double.IsNaN(dist) || double.IsInfinity(dist) || dist > float.MaxValue)
        {
            distance = 0f;
            return false;
        }

        distance = (float)dist;
        return true;
    }

    private static void ExpandAabb(Vec3 v,
        ref float minX, ref float maxX,
        ref float minY, ref float maxY,
        ref float minZ, ref float maxZ)
    {
        if (v.X < minX) minX = v.X; if (v.X > maxX) maxX = v.X;
        if (v.Y < minY) minY = v.Y; if (v.Y > maxY) maxY = v.Y;
        if (v.Z < minZ) minZ = v.Z; if (v.Z > maxZ) maxZ = v.Z;
    }

    // ── STL parsing ──────────────────────────────────────────────────────────

    private static async Task<List<Triangle3D>?> ParseStlAsync(string path)
    {
        await using var stream = File.OpenRead(path);

        var header = new byte[80];
        int read = 0;
        while (read < 80)
        {
            int n = await stream.ReadAsync(header.AsMemory(read, 80 - read));
            if (n == 0) return null;
            read += n;
        }

        var countBuf = new byte[4];
        await stream.ReadExactlyAsync(countBuf);
        uint triCount = BitConverter.ToUInt32(countBuf);

        // Distinguish binary vs ASCII by exact file-size check (more reliable than text heuristics).
        // Binary STL: exactly 84 + triCount*50 bytes.
        long expectedBinarySize = 84L + (long)triCount * 50;
        if (stream.Length == expectedBinarySize && triCount > 0)
            return await ParseBinaryStlAsync(stream, triCount);

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
                BitConverter.ToSingle(buf, 0), BitConverter.ToSingle(buf, 4), BitConverter.ToSingle(buf, 8));
            var v0 = new Vec3(
                BitConverter.ToSingle(buf, 12), BitConverter.ToSingle(buf, 16), BitConverter.ToSingle(buf, 20));
            var v1 = new Vec3(
                BitConverter.ToSingle(buf, 24), BitConverter.ToSingle(buf, 28), BitConverter.ToSingle(buf, 32));
            var v2 = new Vec3(
                BitConverter.ToSingle(buf, 36), BitConverter.ToSingle(buf, 40), BitConverter.ToSingle(buf, 44));
            // buf[48..49] = attribute word, already consumed
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

    // ── OBJ parsing ──────────────────────────────────────────────────────────

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
