using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class SupportSeparationServiceTests
{
    private readonly SupportSeparationService _sut = new();

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// <summary>Creates a simple flat unit square split into 2 triangles sharing an edge.</summary>
    private static List<Triangle3D> MakeSquare(float xOffset = 0, float zOffset = 0)
    {
        var v0 = new Vec3(xOffset + 0, 0, zOffset + 0);
        var v1 = new Vec3(xOffset + 1, 0, zOffset + 0);
        var v2 = new Vec3(xOffset + 1, 0, zOffset + 1);
        var v3 = new Vec3(xOffset + 0, 0, zOffset + 1);
        var up = new Vec3(0, 1, 0);
        return [new(v0, v1, v2, up), new(v0, v2, v3, up)];
    }

    /// <summary>
    /// Creates a mesh with a large body component and a small detached component.
    /// Body has <paramref name="bodySquares"/> squares; supports have <paramref name="supportSquares"/>.
    /// </summary>
    private static List<Triangle3D> MakeBodyAndSupports(int bodySquares, int supportSquares)
    {
        var triangles = new List<Triangle3D>();
        // Body: squares placed side-by-side in X (touching edges => same component)
        for (int i = 0; i < bodySquares; i++)
            triangles.AddRange(MakeSquare(xOffset: i * 1, zOffset: 0));
        // Supports: squares far away in Z so they don't share vertices with body
        for (int i = 0; i < supportSquares; i++)
            triangles.AddRange(MakeSquare(xOffset: i * 1, zOffset: 1000));
        return triangles;
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    [Fact]
    public void Separate_EmptyInput_ReturnsEmptyModelAndNullSupports()
    {
        var (model, supports) = _sut.Separate([]);
        Assert.Empty(model);
        Assert.Null(supports);
    }

    // ── Single component ──────────────────────────────────────────────────────

    [Fact]
    public void Separate_SingleConnectedComponent_ReturnsAllAsModel()
    {
        var triangles = MakeSquare();
        var (model, supports) = _sut.Separate(triangles);
        Assert.Equal(2, model.Count);
        Assert.Null(supports);
    }

    [Fact]
    public void Separate_LargeConnectedMesh_ReturnsAllAsModel()
    {
        // 50 squares sharing edges = one big component, no small components
        var triangles = MakeBodyAndSupports(50, 0);
        var (model, supports) = _sut.Separate(triangles);
        Assert.Equal(100, model.Count);
        Assert.Null(supports);
    }

    // ── Two components ────────────────────────────────────────────────────────

    [Fact]
    public void Separate_SmallDetachedComponent_IsClassifiedAsSupports()
    {
        // Body: 20 squares (40 tris), supports: 1 square (2 tris) — 5% of body, < 20% threshold
        var triangles = MakeBodyAndSupports(20, 1);
        var (model, supports) = _sut.Separate(triangles);

        Assert.NotNull(supports);
        Assert.Equal(2, supports!.Count);
        Assert.Equal(40, model.Count);
    }

    [Fact]
    public void Separate_SupportBelowMinFraction_ReturnsNullForSupports()
    {
        // Body: 200 squares (400 tris), support: 1 square (2 tris)
        // 2/402 ≈ 0.5%, below MinSupportFraction of 1% => null supports
        var triangles = MakeBodyAndSupports(200, 1);
        var (model, supports) = _sut.Separate(triangles);
        Assert.Null(supports);
    }

    [Fact]
    public void Separate_TwoEqualComponents_NeitherIsClassedAsSupport()
    {
        // Two equal-sized detached meshes: neither is < 20% of the other
        var triangles = MakeBodyAndSupports(10, 10);
        var (model, supports) = _sut.Separate(triangles);
        // Both components are equally sized, so no component is below threshold
        Assert.Null(supports);
    }

    [Fact]
    public void Separate_MultipleSmallSupports_AllClassifiedAsSupports()
    {
        // Body: 30 squares, supports: 3 individual squares (well below 20% threshold)
        var body = Enumerable.Range(0, 30).SelectMany(i => MakeSquare(xOffset: i, zOffset: 0)).ToList();
        var supports1 = MakeSquare(xOffset: 0, zOffset: 1000);
        var supports2 = MakeSquare(xOffset: 0, zOffset: 2000);
        var supports3 = MakeSquare(xOffset: 0, zOffset: 3000);
        var triangles = body.Concat(supports1).Concat(supports2).Concat(supports3).ToList();

        var (model, supports) = _sut.Separate(triangles);

        Assert.NotNull(supports);
        Assert.Equal(6, supports!.Count);   // 3 support squares × 2 triangles each
        Assert.Equal(60, model.Count);      // 30 body squares × 2 triangles each
    }

    [Fact]
    public void Separate_PointContactSupportTip_IsClassifiedAsSupport()
    {
        // Body: large detached component. Tip: tiny component touching body at one vertex only.
        var body = Enumerable.Range(0, 20).SelectMany(i => MakeSquare(xOffset: i, zOffset: 0)).ToList();
        var up = new Vec3(0, 1, 0);
        var tip = new List<Triangle3D>
        {
            new(new Vec3(0, 0, 0), new Vec3(-0.2f, 0, 0), new Vec3(0, 0.2f, 0), up),
            new(new Vec3(0, 0, 0), new Vec3(0, 0.2f, 0), new Vec3(0, 0, -0.2f), up),
        };
        var triangles = body.Concat(tip).ToList();

        var (model, supports) = _sut.Separate(triangles);

        Assert.NotNull(supports);
        Assert.Equal(2, supports!.Count);
        Assert.Equal(40, model.Count);
    }

    // ── Quantization tolerance ────────────────────────────────────────────────

    [Fact]
    public void Separate_SharedEdgeWithinQuantizationTolerance_TreatedAsConnected()
    {
        // Two triangles sharing an edge where one endpoint differs by < 0.01mm.
        // Quantization should collapse that endpoint and keep the component connected.
        var v0 = new Vec3(0, 0, 0);
        var v1 = new Vec3(1, 0, 0);
        var v2 = new Vec3(0, 0, 1);
        var v3 = new Vec3(0, 0, 1.001f); // within 0.01mm of v2 → same quantized cell
        var v4 = new Vec3(1, 0, 0);
        var v5 = new Vec3(1, 0, 1);
        var up = new Vec3(0, 1, 0);

        var triangles = new List<Triangle3D>
        {
            new(v0, v1, v2, up),
            new(v3, v4, v5, up),
        };

        var (model, supports) = _sut.Separate(triangles);
        Assert.Equal(2, model.Count);
        Assert.Null(supports);
    }
}
