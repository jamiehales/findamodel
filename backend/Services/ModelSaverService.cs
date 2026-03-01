namespace findamodel.Services;

/// <summary>
/// Serialises geometry into 3D model file formats.
/// Currently supports binary STL only.
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
}
