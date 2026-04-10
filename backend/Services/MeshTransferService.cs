using System.Buffers.Binary;

namespace findamodel.Services;

/// <summary>
/// Serialises centred Y-up mesh geometry into a compact binary format optimized for
/// frontend transfer: 16-bit quantised indexed positions plus 16/32-bit triangle indices.
/// </summary>
public class MeshTransferService
{
    public const string ContentType = "application/vnd.findamodel.mesh";

    private const uint Magic = 0x48534D46; // "FMSH" little-endian
    private const byte Version = 1;
    private const byte QuantizationBits = 16;
    private const int HeaderSize = 56;

    public byte[] Encode(LoadedGeometry geometry)
    {
        ArgumentNullException.ThrowIfNull(geometry);

        float dimX = geometry.DimensionXMm;
        float dimY = geometry.DimensionYMm;
        float dimZ = geometry.DimensionZMm;

        float minX = -dimX * 0.5f;
        float minY = 0f;
        float minZ = -dimZ * 0.5f;

        var vertices = new List<QuantizedVertex>();
        var vertexMap = new Dictionary<QuantizedVertex, int>();
        var indices = new List<int>(geometry.Triangles.Count * 3);

        int GetOrAdd(Vec3 v)
        {
            var q = new QuantizedVertex(
                Quantize(v.X, minX, dimX),
                Quantize(v.Y, minY, dimY),
                Quantize(v.Z, minZ, dimZ));

            if (!vertexMap.TryGetValue(q, out var index))
            {
                index = vertices.Count;
                vertices.Add(q);
                vertexMap[q] = index;
            }

            return index;
        }

        foreach (var tri in geometry.Triangles)
        {
            indices.Add(GetOrAdd(tri.V0));
            indices.Add(GetOrAdd(tri.V1));
            indices.Add(GetOrAdd(tri.V2));
        }

        byte indexElementSize = vertices.Count <= ushort.MaxValue ? (byte)2 : (byte)4;
        int positionsByteLength = vertices.Count * 3 * sizeof(ushort);
        int indexOffset = Align4(HeaderSize + positionsByteLength);
        int indicesByteLength = indices.Count * indexElementSize;

        var buffer = new byte[indexOffset + indicesByteLength];
        var span = buffer.AsSpan();

        BinaryPrimitives.WriteUInt32LittleEndian(span[0..4], Magic);
        span[4] = Version;
        span[5] = indexElementSize;
        span[6] = QuantizationBits;
        span[7] = 0;
        BinaryPrimitives.WriteUInt32LittleEndian(span[8..12], (uint)vertices.Count);
        BinaryPrimitives.WriteUInt32LittleEndian(span[12..16], (uint)geometry.Triangles.Count);
        WriteSingle(span[16..20], dimX);
        WriteSingle(span[20..24], dimY);
        WriteSingle(span[24..28], dimZ);
        WriteSingle(span[28..32], geometry.SphereCentre.X);
        WriteSingle(span[32..36], geometry.SphereCentre.Y);
        WriteSingle(span[36..40], geometry.SphereCentre.Z);
        WriteSingle(span[40..44], geometry.SphereRadius);
        BinaryPrimitives.WriteUInt32LittleEndian(span[44..48], (uint)positionsByteLength);
        BinaryPrimitives.WriteUInt32LittleEndian(span[48..52], (uint)indexOffset);
        BinaryPrimitives.WriteUInt32LittleEndian(span[52..56], (uint)indicesByteLength);

        int offset = HeaderSize;
        foreach (var vertex in vertices)
        {
            BinaryPrimitives.WriteUInt16LittleEndian(span[offset..(offset + 2)], vertex.X);
            offset += 2;
            BinaryPrimitives.WriteUInt16LittleEndian(span[offset..(offset + 2)], vertex.Y);
            offset += 2;
            BinaryPrimitives.WriteUInt16LittleEndian(span[offset..(offset + 2)], vertex.Z);
            offset += 2;
        }

        offset = indexOffset;
        if (indexElementSize == 2)
        {
            foreach (var index in indices)
            {
                BinaryPrimitives.WriteUInt16LittleEndian(span[offset..(offset + 2)], (ushort)index);
                offset += 2;
            }
        }
        else
        {
            foreach (var index in indices)
            {
                BinaryPrimitives.WriteUInt32LittleEndian(span[offset..(offset + 4)], (uint)index);
                offset += 4;
            }
        }

        return buffer;
    }

    private static ushort Quantize(float value, float min, float range)
    {
        if (range <= 1e-8f) return 0;

        float normalized = (value - min) / range;
        normalized = Math.Clamp(normalized, 0f, 1f);
        return (ushort)Math.Clamp((int)MathF.Round(normalized * ushort.MaxValue), 0, ushort.MaxValue);
    }

    private static void WriteSingle(Span<byte> destination, float value)
        => BinaryPrimitives.WriteSingleLittleEndian(destination, value);

    private static int Align4(int value) => (value + 3) & ~3;

    private readonly record struct QuantizedVertex(ushort X, ushort Y, ushort Z);
}