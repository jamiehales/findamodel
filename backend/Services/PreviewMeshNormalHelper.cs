namespace findamodel.Services;

internal static class PreviewMeshNormalHelper
{
    public static Vec3 Compute(Triangle3D triangle)
    {
        var faceNormal = (triangle.V1 - triangle.V0).Cross(triangle.V2 - triangle.V0);
        if (faceNormal.LengthSq >= 1e-12f)
            return faceNormal.Normalized;

        return triangle.Normal.Normalized;
    }
}