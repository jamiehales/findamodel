namespace findamodel.Services;

public readonly record struct Vec3(float X, float Y, float Z)
{
    public static readonly Vec3 Up = new(0, 1, 0);

    public static Vec3 operator +(Vec3 a, Vec3 b) => new(a.X + b.X, a.Y + b.Y, a.Z + b.Z);
    public static Vec3 operator -(Vec3 a, Vec3 b) => new(a.X - b.X, a.Y - b.Y, a.Z - b.Z);
    public static Vec3 operator *(Vec3 v, float s) => new(v.X * s, v.Y * s, v.Z * s);
    public static Vec3 operator *(float s, Vec3 v) => v * s;
    public static Vec3 operator -(Vec3 v)          => new(-v.X, -v.Y, -v.Z);

    public float Dot(Vec3 b)  => X * b.X + Y * b.Y + Z * b.Z;
    public Vec3 Cross(Vec3 b) => new(Y * b.Z - Z * b.Y, Z * b.X - X * b.Z, X * b.Y - Y * b.X);

    public float LengthSq => X * X + Y * Y + Z * Z;
    public float Length   => MathF.Sqrt(LengthSq);
    public Vec3 Normalized
    {
        get { float l = Length; return l < 1e-8f ? Up : this * (1f / l); }
    }
}

public readonly record struct Triangle3D(Vec3 V0, Vec3 V1, Vec3 V2, Vec3 Normal);

/// <summary>
/// The result of loading a 3D model through <see cref="ModelLoaderService"/>.
///
/// Coordinate system: Y-up (Z-up source files are rotated on load via rotateX(π/2) + rotateZ(π)).
/// Units: millimetres (assumed — STL/OBJ carry no unit metadata; scale factor = 1.0).
/// Origin: X/Z centred at 0, base face (minimum Y) sitting at Y = 0.
/// </summary>
public class LoadedGeometry
{
    /// <summary>Triangles in Y-up, mm, centred coordinates.</summary>
    public required List<Triangle3D> Triangles { get; init; }

    /// <summary>Centre of the bounding sphere of the centred model (= AABB centre = (0, dimY/2, 0)).</summary>
    public required Vec3 SphereCentre { get; init; }

    /// <summary>Radius of the bounding sphere in mm (max vertex distance from SphereCentre).</summary>
    public required float SphereRadius { get; init; }

    /// <summary>Width of the axis-aligned bounding box in mm (X axis).</summary>
    public required float DimensionXMm { get; init; }

    /// <summary>Height of the axis-aligned bounding box in mm (Y axis).</summary>
    public required float DimensionYMm { get; init; }

    /// <summary>Depth of the axis-aligned bounding box in mm (Z axis).</summary>
    public required float DimensionZMm { get; init; }
}
