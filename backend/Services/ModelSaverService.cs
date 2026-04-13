using System.Globalization;
using System.IO.Compression;
using System.Numerics;
using System.Text;

namespace findamodel.Services;

/// <summary>
/// Serialises geometry into 3D model file formats.
/// Supports binary STL and 3MF (with instancing).
///
/// Triangles are written as-is - the caller is responsible for any coordinate-system
/// conversion before passing them in.
///
/// Thread-safe: stateless; safe for singleton DI registration.
/// </summary>
public class ModelSaverService
{
    /// <summary>
    /// Serialises the supplied triangles as a binary STL and returns the raw bytes.
    /// Binary STL format: 80-byte ASCII header + uint32 triangle count + 50 bytes per triangle
    /// (float32[3] normal, float32[3] v0, float32[3] v1, float32[3] v2, uint16 attribute = 0).
    /// </summary>
    /// <param name="triangles">Triangles to write. Each triangle must carry a pre-computed normal.</param>
    /// <param name="headerText">Optional ASCII text embedded in the 80-byte header (truncated at 79 chars).</param>
    public byte[] SaveStl(IReadOnlyList<Triangle3D> triangles, string? headerText = null)
    {
        // 80-byte header + 4-byte count + 50 bytes × N triangles
        var buffer = new byte[84 + (long)triangles.Count * 50];
        var span = buffer.AsSpan();

        // Header (80 bytes) - must not start with "solid" to avoid ASCII mis-detection
        if (!string.IsNullOrEmpty(headerText))
        {
            var encoded = System.Text.Encoding.ASCII.GetBytes(headerText);
            int copyLen = Math.Min(encoded.Length, 79); // leave at least one null byte
            encoded.AsSpan(0, copyLen).CopyTo(span.Slice(0, copyLen));
        }

        // Triangle count (bytes 80-83, little-endian uint32)
        BitConverter.TryWriteBytes(span.Slice(80, 4), (uint)triangles.Count);

        int offset = 84;
        foreach (var tri in triangles)
        {
            WriteVec3(span, ref offset, tri.Normal);
            WriteVec3(span, ref offset, tri.V0);
            WriteVec3(span, ref offset, tri.V1);
            WriteVec3(span, ref offset, tri.V2);
            span[offset] = 0; // attribute word (2 bytes, always 0)
            span[offset + 1] = 0;
            offset += 2;
        }

        return buffer;
    }

    private static void WriteVec3(Span<byte> buf, ref int offset, Vec3 v)
    {
        BitConverter.TryWriteBytes(buf.Slice(offset, 4), v.X);
        BitConverter.TryWriteBytes(buf.Slice(offset + 4, 4), v.Y);
        BitConverter.TryWriteBytes(buf.Slice(offset + 8, 4), v.Z);
        offset += 12;
    }

    /// <summary>
    /// Serialises the supplied geometry as a 3MF package and returns the raw bytes.
    ///
    /// Each entry in <paramref name="objects"/> defines one mesh resource (Z-up coordinates,
    /// base position with no placement transform applied).  Each entry in
    /// <paramref name="items"/> references one of those resources and carries the 3MF
    /// transform string that positions it on the build plate.  Multiple items may reference
    /// the same object ID, which is how 3MF instancing works.
    /// </summary>
    public byte[] Save3mf(
        IReadOnlyList<(int Id, IReadOnlyList<Triangle3D> Triangles)> objects,
        IReadOnlyList<(int ObjectId, string Transform)> items)
    {
        var modelXml = Build3dModelXml(objects, items);

        using var ms = new MemoryStream();
        using (var zip = new ZipArchive(ms, ZipArchiveMode.Create, leaveOpen: true))
        {
            // Alphabetical by Extension, matching lib3mf's default registration set.
            AddZipEntry(zip, "[Content_Types].xml",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" +
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">" +
                "<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>" +
                "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>" +
                "<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>" +
                "<Default Extension=\"png\" ContentType=\"image/png\"/>" +
                "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>" +
                "<Default Extension=\"texture\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodeltexture\"/>" +
                "</Types>");

            // Attribute order: Type / Target / Id - matches lib3mf output.
            AddZipEntry(zip, "_rels/.rels",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" +
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">" +
                "<Relationship Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\" Target=\"/3D/3dmodel.model\" Id=\"rel0\"/>" +
                "</Relationships>");

            AddZipEntry(zip, "3D/3dmodel.model", modelXml);
        }

        return ms.ToArray();
    }

    private static void AddZipEntry(ZipArchive zip, string name, string content)
    {
        var entry = zip.CreateEntry(name, CompressionLevel.Optimal);
        using var stream = entry.Open();
        var bytes = Encoding.UTF8.GetBytes(content);
        stream.Write(bytes, 0, bytes.Length);
    }

    private static string Build3dModelXml(
        IReadOnlyList<(int Id, IReadOnlyList<Triangle3D> Triangles)> objects,
        IReadOnlyList<(int ObjectId, string Transform)> items)
    {
        var sb = new StringBuilder();
        sb.Append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        // xmlns first, then unit / xml:lang, then all extension namespaces - matching lib3mf order.
        sb.Append(
            "<model" +
            " xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\"" +
            " unit=\"millimeter\" xml:lang=\"en-US\"" +
            " xmlns:m=\"http://schemas.microsoft.com/3dmanufacturing/material/2015/02\"" +
            " xmlns:p=\"http://schemas.microsoft.com/3dmanufacturing/production/2015/06\"" +
            " xmlns:b=\"http://schemas.microsoft.com/3dmanufacturing/beamlattice/2017/02\"" +
            " xmlns:s=\"http://schemas.microsoft.com/3dmanufacturing/slice/2015/07\"" +
            " xmlns:t=\"http://schemas.microsoft.com/3dmanufacturing/trianglesets/2021/07\"" +
            " xmlns:sc=\"http://schemas.microsoft.com/3dmanufacturing/securecontent/2019/04\"" +
            " xmlns:v=\"http://schemas.3mf.io/3dmanufacturing/volumetric/2022/01\"" +
            " xmlns:i=\"http://schemas.3mf.io/3dmanufacturing/implicit/2023/12\">");
        sb.Append("<resources>");

        // Base mesh objects - not referenced directly in <build>.
        foreach (var (id, triangles) in objects)
        {
            // Deduplicate vertices so shared corners are written once.
            var vertices = new List<Vec3>();
            var vertexMap = new Dictionary<Vec3, int>();
            int GetOrAdd(Vec3 v)
            {
                if (!vertexMap.TryGetValue(v, out var idx))
                {
                    idx = vertices.Count;
                    vertices.Add(v);
                    vertexMap[v] = idx;
                }
                return idx;
            }

            var triIndices = new (int V0, int V1, int V2)[triangles.Count];
            for (int i = 0; i < triangles.Count; i++)
            {
                var tri = triangles[i];
                triIndices[i] = (GetOrAdd(tri.V0), GetOrAdd(tri.V1), GetOrAdd(tri.V2));
            }

            sb.Append($"<object id=\"{id}\" type=\"model\" p:UUID=\"{Guid.NewGuid()}\"><mesh><vertices>");
            foreach (var v in vertices)
                AppendVertex(sb, v);
            sb.Append("</vertices><triangles>");
            foreach (var (v0, v1, v2) in triIndices)
                sb.Append($"<triangle v1=\"{v0}\" v2=\"{v1}\" v3=\"{v2}\"/>");
            sb.Append("</triangles></mesh></object>");
        }

        // Per-placement component wrapper objects.
        //
        // The 3MF Core Spec (§5.2) forbids two <item> elements referencing the same objectid.
        // Instancing is achieved by creating a unique wrapper object per placement that
        // references the shared mesh via <component objectid="..." transform="..."/>.
        // The <build> then references only the unique wrapper objects.
        int instanceId = objects.Max(o => o.Id) + 1;
        var instanceIds = new List<int>(items.Count);
        foreach (var (objectId, transform) in items)
        {
            sb.Append($"<object id=\"{instanceId}\" type=\"model\" p:UUID=\"{Guid.NewGuid()}\">" +
                      $"<components><component objectid=\"{objectId}\" transform=\"{transform}\" p:UUID=\"{Guid.NewGuid()}\"/></components></object>");
            instanceIds.Add(instanceId++);
        }

        sb.Append($"</resources><build p:UUID=\"{Guid.NewGuid()}\">");
        foreach (var id in instanceIds)
            sb.Append($"<item objectid=\"{id}\" p:UUID=\"{Guid.NewGuid()}\"/>");
        sb.Append("</build></model>");

        return sb.ToString();
    }

    /// <summary>
    /// Formats a float coordinate to 6 decimal places, matching lib3mf's output precision.
    /// Values that round to zero (|v| &lt; 5e-7) are emitted as "0" rather than "0.000000".
    /// </summary>
    private static string FormatCoord(float v)
    {
        float r = MathF.Round(v, 6);
        return r == 0f ? "0" : r.ToString("F6", CultureInfo.InvariantCulture);
    }

    private static void AppendVertex(StringBuilder sb, Vec3 v)
    {
        sb.Append($"<vertex x=\"{FormatCoord(v.X)}\" y=\"{FormatCoord(v.Y)}\" z=\"{FormatCoord(v.Z)}\"/>");
    }

    // -------------------------------------------------------------------------
    // GLB (binary glTF 2.0) output - via SharpGLTF.Toolkit
    // -------------------------------------------------------------------------

    /// <summary>
    /// Serialises the supplied geometry as a GLB (binary glTF 2.0) and returns the raw bytes.
    ///
    /// Geometry must be in Y-up coordinates - glTF's native coordinate system - so no
    /// axis conversion is applied here.  Each entry in <paramref name="objects"/> defines
    /// one mesh that is stored once.  Each entry in <paramref name="items"/> creates a glTF
    /// node that references one of those meshes; passing the same mesh builder to multiple
    /// <c>AddRigidMesh</c> calls is how SharpGLTF achieves node-level instancing.
    ///
    /// Meshes use flat shading: each triangle's face normal is replicated across all three
    /// of its vertices so the NORMAL attribute carries per-face shading correctly.
    /// </summary>
    public byte[] SaveGlb(
        IReadOnlyList<(int Id, IReadOnlyList<Triangle3D> Triangles)> objects,
        IReadOnlyList<(int ObjectId, Matrix4x4 Transform)> items)
    {
        // Build one MeshBuilder per unique object.
        var meshByObjectId = new Dictionary<int, SharpGLTF.Geometry.MeshBuilder<
            SharpGLTF.Geometry.VertexTypes.VertexPositionNormal>>(objects.Count);

        foreach (var (id, triangles) in objects)
        {
            var mb = new SharpGLTF.Geometry.MeshBuilder<
                SharpGLTF.Geometry.VertexTypes.VertexPositionNormal>($"mesh_{id}");
            var prim = mb.UsePrimitive(SharpGLTF.Materials.MaterialBuilder.CreateDefault());

            foreach (var tri in triangles)
            {
                var n = new Vector3(tri.Normal.X, tri.Normal.Y, tri.Normal.Z);
                prim.AddTriangle(
                    new SharpGLTF.Geometry.VertexTypes.VertexPositionNormal(
                        new Vector3(tri.V0.X, tri.V0.Y, tri.V0.Z), n),
                    new SharpGLTF.Geometry.VertexTypes.VertexPositionNormal(
                        new Vector3(tri.V1.X, tri.V1.Y, tri.V1.Z), n),
                    new SharpGLTF.Geometry.VertexTypes.VertexPositionNormal(
                        new Vector3(tri.V2.X, tri.V2.Y, tri.V2.Z), n));
            }

            meshByObjectId[id] = mb;
        }

        // Add one rigid node per placement, reusing the same MeshBuilder for instancing.
        var scene = new SharpGLTF.Scenes.SceneBuilder();
        foreach (var (objectId, transform) in items)
            scene.AddRigidMesh(meshByObjectId[objectId], transform);

        var model = scene.ToGltf2();
        using var ms = new MemoryStream();
        model.WriteGLB(ms);
        return ms.ToArray();
    }
}
