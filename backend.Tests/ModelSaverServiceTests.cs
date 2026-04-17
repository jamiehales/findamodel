using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ModelSaverServiceTests
{
    [Fact]
    public void SaveStl_EnumerableOverload_MatchesListOutput()
    {
        var triangles = new List<Triangle3D>
        {
            new(new Vec3(0, 0, 0), new Vec3(10, 0, 0), new Vec3(0, 10, 0), new Vec3(0, 0, 1)),
            new(new Vec3(0, 0, 0), new Vec3(0, 10, 0), new Vec3(0, 0, 10), new Vec3(1, 0, 0)),
        };

        var saver = new ModelSaverService();
        var expected = saver.SaveStl(triangles, "stream test");
        var actual = saver.SaveStl(triangles.Count, triangles, "stream test");

        Assert.Equal(expected, actual);
    }

    [Fact]
    public void SaveStl_StreamOverload_MatchesByteArrayOutput()
    {
        var triangles = new List<Triangle3D>
        {
            new(new Vec3(0, 0, 0), new Vec3(10, 0, 0), new Vec3(0, 10, 0), new Vec3(0, 0, 1)),
            new(new Vec3(0, 0, 0), new Vec3(0, 10, 0), new Vec3(0, 0, 10), new Vec3(1, 0, 0)),
        };

        var saver = new ModelSaverService();
        var expected = saver.SaveStl(triangles, "stream test");

        using var ms = new MemoryStream();
        saver.SaveStl(ms, triangles.Count, triangles, "stream test");

        Assert.Equal(expected, ms.ToArray());
    }
}
