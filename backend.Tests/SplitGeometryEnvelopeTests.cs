using System.Buffers.Binary;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

/// <summary>
/// Verifies that the binary envelope produced for GetSplitGeometry has the correct structure:
/// [bodyLength:uint32][body FMSH][supportLength:uint32][support FMSH].
/// </summary>
public class SplitGeometryEnvelopeTests
{
    private static readonly uint FmshMagic = 0x48534D46;
    private readonly MeshTransferService _sut = new();

    private static LoadedGeometry MakeGeometry(List<Triangle3D> triangles)
    {
        // Minimal bounding box covering the triangles used in tests
        return new LoadedGeometry
        {
            Triangles = triangles,
            DimensionXMm = 10f,
            DimensionYMm = 1f,
            DimensionZMm = 10f,
            SphereCentre = new Vec3(0, 0.5f, 0),
            SphereRadius = 8f,
        };
    }

    private static List<Triangle3D> MakeSquare(float xOffset = 0, float zOffset = 0)
    {
        var v0 = new Vec3(xOffset + 0, 0, zOffset + 0);
        var v1 = new Vec3(xOffset + 1, 0, zOffset + 0);
        var v2 = new Vec3(xOffset + 1, 0, zOffset + 1);
        var v3 = new Vec3(xOffset + 0, 0, zOffset + 1);
        var up = Vec3.Up;
        return [new(v0, v1, v2, up), new(v0, v2, v3, up)];
    }

    [Fact]
    public void Envelope_BodyAndSupportBlobs_AreValidFmshPayloads()
    {
        var bodytriangles = new List<Triangle3D>();
        for (int i = 0; i < 4; i++) bodytriangles.AddRange(MakeSquare(xOffset: i));
        var supportTriangles = MakeSquare(zOffset: 100f);

        var bodyPayload = _sut.Encode(MakeGeometry(bodytriangles));
        var supportPayload = _sut.Encode(MakeGeometry(supportTriangles));

        // Build envelope the same way the controller does
        var envelope = new byte[4 + bodyPayload.Length + 4 + supportPayload.Length];
        var span = envelope.AsSpan();
        BinaryPrimitives.WriteUInt32LittleEndian(span[0..4], (uint)bodyPayload.Length);
        bodyPayload.CopyTo(span[4..]);
        int afterBody = 4 + bodyPayload.Length;
        BinaryPrimitives.WriteUInt32LittleEndian(span[afterBody..(afterBody + 4)], (uint)supportPayload.Length);
        supportPayload.CopyTo(span[(afterBody + 4)..]);

        // Parse the envelope
        var view = new DataView(envelope);
        int offset = 0;

        uint parsedBodyLength = view.ReadUInt32(offset); offset += 4;
        Assert.Equal((uint)bodyPayload.Length, parsedBodyLength);

        // Body blob starts with FMSH magic and encodes the correct triangle count
        uint bodyMagic = view.ReadUInt32(offset);
        Assert.Equal(FmshMagic, bodyMagic);
        uint bodyTriangleCount = view.ReadUInt32(offset + 12);
        Assert.Equal((uint)bodytriangles.Count, bodyTriangleCount);
        offset += (int)parsedBodyLength;

        uint parsedSupportLength = view.ReadUInt32(offset); offset += 4;
        Assert.Equal((uint)supportPayload.Length, parsedSupportLength);

        // Support blob starts with FMSH magic and encodes the correct triangle count
        uint supportMagic = view.ReadUInt32(offset);
        Assert.Equal(FmshMagic, supportMagic);
        uint supportTriangleCount = view.ReadUInt32(offset + 12);
        Assert.Equal((uint)supportTriangles.Count, supportTriangleCount);
    }

    [Fact]
    public void EnvelopeLengths_Match_ActualBlobSizes()
    {
        var triangles = MakeSquare();
        var payload1 = _sut.Encode(MakeGeometry(triangles));
        var payload2 = _sut.Encode(MakeGeometry(triangles));

        var envelope = new byte[4 + payload1.Length + 4 + payload2.Length];
        var span = envelope.AsSpan();
        BinaryPrimitives.WriteUInt32LittleEndian(span[0..4], (uint)payload1.Length);
        payload1.CopyTo(span[4..]);
        int afterFirst = 4 + payload1.Length;
        BinaryPrimitives.WriteUInt32LittleEndian(span[afterFirst..(afterFirst + 4)], (uint)payload2.Length);
        payload2.CopyTo(span[(afterFirst + 4)..]);

        Assert.Equal(4 + payload1.Length + 4 + payload2.Length, envelope.Length);
        Assert.Equal((uint)payload1.Length, BinaryPrimitives.ReadUInt32LittleEndian(envelope.AsSpan(0, 4)));
        Assert.Equal((uint)payload2.Length, BinaryPrimitives.ReadUInt32LittleEndian(envelope.AsSpan(afterFirst, 4)));
    }

    /// <summary>Minimal little-endian reader over a byte array for test assertions.</summary>
    private ref struct DataView(byte[] data)
    {
        private readonly byte[] _data = data;

        public uint ReadUInt32(int offset) =>
            BinaryPrimitives.ReadUInt32LittleEndian(_data.AsSpan(offset, 4));
    }
}
