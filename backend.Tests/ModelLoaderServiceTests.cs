using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class ModelLoaderServiceTests
{
    [Fact]
    public async Task LoadModelAsync_LoadsBinaryStl()
    {
        var path = Path.Combine(Path.GetTempPath(), $"findamodel-binary-stl-{Guid.NewGuid():N}.stl");
        try
        {
            await using (var stream = File.Create(path))
            {
                await stream.WriteAsync(new byte[80]);
                await stream.WriteAsync(BitConverter.GetBytes((uint)1));

                var triangle = new byte[50];
                BitConverter.GetBytes(0f).CopyTo(triangle, 0);
                BitConverter.GetBytes(0f).CopyTo(triangle, 4);
                BitConverter.GetBytes(1f).CopyTo(triangle, 8);

                BitConverter.GetBytes(0f).CopyTo(triangle, 12);
                BitConverter.GetBytes(0f).CopyTo(triangle, 16);
                BitConverter.GetBytes(0f).CopyTo(triangle, 20);

                BitConverter.GetBytes(10f).CopyTo(triangle, 24);
                BitConverter.GetBytes(0f).CopyTo(triangle, 28);
                BitConverter.GetBytes(0f).CopyTo(triangle, 32);

                BitConverter.GetBytes(0f).CopyTo(triangle, 36);
                BitConverter.GetBytes(10f).CopyTo(triangle, 40);
                BitConverter.GetBytes(0f).CopyTo(triangle, 44);

                await stream.WriteAsync(triangle);
            }

            var sut = new ModelLoaderService(NullLoggerFactory.Instance);
            var geometry = await sut.LoadModelAsync(path, "stl");

            Assert.NotNull(geometry);
            Assert.Single(geometry!.Triangles);
            Assert.True(geometry.DimensionXMm > 0);
            Assert.True(geometry.DimensionZMm > 0);
            Assert.True(float.IsFinite(geometry.SphereRadius));
        }
        finally
        {
            if (File.Exists(path))
                File.Delete(path);
        }
    }

    [Fact]
    public async Task LoadModelAsync_IgnoresTrianglesWithNaNVertices()
    {
        var path = Path.Combine(Path.GetTempPath(), $"findamodel-nan-{Guid.NewGuid():N}.obj");
        try
        {
            await File.WriteAllTextAsync(path, """
                v 0 0 0
                v 10 0 0
                v 0 10 0
                v NaN 0 0
                f 1 2 3
                f 1 4 3
                """);

            var sut = new ModelLoaderService(NullLoggerFactory.Instance);
            var geometry = await sut.LoadModelAsync(path, "obj");

            Assert.NotNull(geometry);
            Assert.Single(geometry!.Triangles);
            Assert.True(float.IsFinite(geometry.SphereRadius));
            Assert.True(float.IsFinite(geometry.SphereCentre.X));
            Assert.True(float.IsFinite(geometry.SphereCentre.Y));
            Assert.True(float.IsFinite(geometry.SphereCentre.Z));
        }
        finally
        {
            if (File.Exists(path))
                File.Delete(path);
        }
    }

    [Fact]
    public async Task LoadModelAsync_ReturnsNull_WhenAllGeometryIsNonFinite()
    {
        var path = Path.Combine(Path.GetTempPath(), $"findamodel-all-invalid-{Guid.NewGuid():N}.obj");
        try
        {
            await File.WriteAllTextAsync(path, """
                v NaN 0 0
                v NaN 1 0
                v NaN 0 1
                f 1 2 3
                """);

            var sut = new ModelLoaderService(NullLoggerFactory.Instance);
            var geometry = await sut.LoadModelAsync(path, "obj");

            Assert.Null(geometry);
        }
        finally
        {
            if (File.Exists(path))
                File.Delete(path);
        }
    }
}
