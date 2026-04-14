using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class PreviewMeshNormalHelperTests
{
    [Fact]
    public void Compute_PrefersFaceNormalOverStoredTriangleNormal()
    {
        var triangle = new Triangle3D(
            new Vec3(0, 0, 0),
            new Vec3(1, 0, 0),
            new Vec3(0, 1, 0),
            new Vec3(0, 0, 1));

        var normal = PreviewMeshNormalHelper.Compute(triangle);

        Assert.Equal(0f, normal.X, 4);
        Assert.Equal(0f, normal.Y, 4);
        Assert.Equal(1f, normal.Z, 4);
    }

    [Fact]
    public void Compute_FallsBackToStoredNormalForDegenerateTriangles()
    {
        var triangle = new Triangle3D(
            new Vec3(0, 0, 0),
            new Vec3(1, 0, 0),
            new Vec3(2, 0, 0),
            new Vec3(0, 0, 2));

        var normal = PreviewMeshNormalHelper.Compute(triangle);

        Assert.Equal(0f, normal.X, 4);
        Assert.Equal(0f, normal.Y, 4);
        Assert.Equal(1f, normal.Z, 4);
    }
}