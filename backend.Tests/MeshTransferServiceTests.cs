using System.Buffers.Binary;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class MeshTransferServiceTests
{
    private static readonly MeshTransferService Transfer = new();

    [Fact]
    public void Encode_DeduplicatesVertices_AndUsesUint16Indices()
    {
        var geometry = CreateTetrahedronGeometry();

        var bytes = Transfer.Encode(geometry);
        var header = MeshHeader.Read(bytes);

        Assert.Equal(4u, header.VertexCount);
        Assert.Equal(4u, header.TriangleCount);
        Assert.Equal(2, header.IndexElementSize);

        var decodedPositions = DecodePositions(bytes, header);
        var expectedPositions = new HashSet<(int X, int Y, int Z)>
        {
            (-5000, 0, -5000),
            (5000, 0, -5000),
            (0, 0, 5000),
            (0, 10000, 0),
        };

        var actualPositions = decodedPositions
            .Select(v => ((int)MathF.Round(v.X * 1000), (int)MathF.Round(v.Y * 1000), (int)MathF.Round(v.Z * 1000)))
            .ToHashSet();

        Assert.Equal(expectedPositions, actualPositions);
    }

    [Fact]
    public void Encode_UsesUint32Indices_WhenVertexCountExceedsUint16()
    {
        const int vertexCount = 65_538;
        var triangles = new List<Triangle3D>(vertexCount / 3);

        for (int i = 0; i < vertexCount; i += 3)
        {
            triangles.Add(new Triangle3D(
                GridVertex(i),
                GridVertex(i + 1),
                GridVertex(i + 2),
                Vec3.Up));
        }

        var geometry = new LoadedGeometry
        {
            Triangles = triangles,
            SphereCentre = new Vec3(0f, 0.5f, 0f),
            SphereRadius = 512f,
            DimensionXMm = 512f,
            DimensionYMm = 1f,
            DimensionZMm = 512f,
        };

        var bytes = Transfer.Encode(geometry);
        var header = MeshHeader.Read(bytes);

        Assert.True(header.VertexCount > ushort.MaxValue);
        Assert.Equal(4, header.IndexElementSize);
    }

    private static LoadedGeometry CreateTetrahedronGeometry()
    {
        var triangles = new List<Triangle3D>
        {
            new(new Vec3(-5, 0, -5), new Vec3(5, 0, -5), new Vec3(0, 0, 5), Vec3.Up),
            new(new Vec3(-5, 0, -5), new Vec3(0, 0, 5), new Vec3(0, 10, 0), Vec3.Up),
            new(new Vec3(5, 0, -5), new Vec3(0, 10, 0), new Vec3(0, 0, 5), Vec3.Up),
            new(new Vec3(-5, 0, -5), new Vec3(0, 10, 0), new Vec3(5, 0, -5), Vec3.Up),
        };

        return new LoadedGeometry
        {
            Triangles = triangles,
            SphereCentre = new Vec3(0f, 5f, 0f),
            SphereRadius = 8.6602545f,
            DimensionXMm = 10f,
            DimensionYMm = 10f,
            DimensionZMm = 10f,
        };
    }

    private static Vec3 GridVertex(int index)
    {
        int x = index & 255;
        int z = (index >> 8) & 255;
        int y = (index >> 16) & 1;
        return new Vec3(x - 128f, y, z - 128f);
    }

    private static List<Vec3> DecodePositions(byte[] bytes, MeshHeader header)
    {
        var result = new List<Vec3>((int)header.VertexCount);
        float xScale = header.DimensionXMm == 0 ? 0 : header.DimensionXMm / ushort.MaxValue;
        float yScale = header.DimensionYMm == 0 ? 0 : header.DimensionYMm / ushort.MaxValue;
        float zScale = header.DimensionZMm == 0 ? 0 : header.DimensionZMm / ushort.MaxValue;
        float minX = -header.DimensionXMm / 2f;
        float minZ = -header.DimensionZMm / 2f;

        for (int i = 0; i < header.VertexCount; i++)
        {
            int offset = MeshHeader.HeaderSize + (i * 6);
            ushort qx = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset, 2));
            ushort qy = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 2, 2));
            ushort qz = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 4, 2));

            result.Add(new Vec3(
                minX + (qx * xScale),
                qy * yScale,
                minZ + (qz * zScale)));
        }

        return result;
    }

    private readonly record struct MeshHeader(
        byte IndexElementSize,
        uint VertexCount,
        uint TriangleCount,
        float DimensionXMm,
        float DimensionYMm,
        float DimensionZMm)
    {
        public const int HeaderSize = 56;

        public static MeshHeader Read(byte[] bytes)
        {
            Assert.True(bytes.Length >= HeaderSize);
            Assert.Equal(0x48534D46u, BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(0, 4)));

            return new MeshHeader(
                bytes[5],
                BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(8, 4)),
                BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(12, 4)),
                BinaryPrimitives.ReadSingleLittleEndian(bytes.AsSpan(16, 4)),
                BinaryPrimitives.ReadSingleLittleEndian(bytes.AsSpan(20, 4)),
                BinaryPrimitives.ReadSingleLittleEndian(bytes.AsSpan(24, 4)));
        }
    }
}