using System.Globalization;
using System.IO.Compression;
using System.Text;

namespace findamodel.Services;

/// <summary>
/// Serialises geometry into 3D model file formats.
/// Supports binary STL and 3MF (with instancing).
///
/// Triangles are written as-is — the caller is responsible for any coordinate-system
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

        // Header (80 bytes) — must not start with "solid" to avoid ASCII mis-detection
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
            span[offset]     = 0; // attribute word (2 bytes, always 0)
            span[offset + 1] = 0;
            offset += 2;
        }

        return buffer;
    }

    private static void WriteVec3(Span<byte> buf, ref int offset, Vec3 v)
    {
        BitConverter.TryWriteBytes(buf.Slice(offset,      4), v.X);
        BitConverter.TryWriteBytes(buf.Slice(offset + 4,  4), v.Y);
        BitConverter.TryWriteBytes(buf.Slice(offset + 8,  4), v.Z);
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
            AddZipEntry(zip, "[Content_Types].xml",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" +
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">" +
                "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>" +
                "<Default Extension=\"model\" ContentType=\"application/vnd.ms-3mf.model\"/>" +
                "</Types>");

            AddZipEntry(zip, "_rels/.rels",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" +
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">" +
                "<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>" +
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
        sb.Append("<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">");
        sb.Append("<resources>");

        // Base mesh objects — not referenced directly in <build>.
        foreach (var (id, triangles) in objects)
        {
            sb.Append($"<object id=\"{id}\" type=\"model\"><mesh><vertices>");
            foreach (var tri in triangles)
            {
                AppendVertex(sb, tri.V0);
                AppendVertex(sb, tri.V1);
                AppendVertex(sb, tri.V2);
            }
            sb.Append("</vertices><triangles>");
            for (int i = 0; i < triangles.Count; i++)
            {
                int b = i * 3;
                sb.Append($"<triangle v1=\"{b}\" v2=\"{b + 1}\" v3=\"{b + 2}\"/>");
            }
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
            sb.Append($"<object id=\"{instanceId}\" type=\"model\"><components><component objectid=\"{objectId}\" transform=\"{transform}\"/></components></object>");
            instanceIds.Add(instanceId++);
        }

        sb.Append("</resources><build>");
        foreach (var id in instanceIds)
            sb.Append($"<item objectid=\"{id}\"/>");
        sb.Append("</build></model>");

        return sb.ToString();
    }

    private static void AppendVertex(StringBuilder sb, Vec3 v)
    {
        sb.Append(string.Create(CultureInfo.InvariantCulture,
            $"<vertex x=\"{v.X:G9}\" y=\"{v.Y:G9}\" z=\"{v.Z:G9}\"/>"));
    }
}
