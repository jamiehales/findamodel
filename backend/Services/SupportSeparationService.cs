namespace findamodel.Services;

/// <summary>
/// Separates a pre-supported mesh into model body and support components using
/// connected-component analysis (union-find on shared quantized edges).
///
/// Heuristic: connected components whose triangle count is below
/// <see cref="SupportSizeThreshold"/> × (largest component size) are classified
/// as supports. This works well for pre-supported STL/OBJ files where supports
/// are physically detached from the main body (no shared edges) or touch only at
/// tiny break-away contact points.
/// </summary>
public class SupportSeparationService
{
    /// <summary>
    /// Components smaller than this fraction of the largest component are supports.
    /// 0.20 = any component with fewer than 20 % of the dominant body's triangles.
    /// </summary>
    private const float SupportSizeThreshold = 0.20f;

    /// <summary>
    /// Minimum fraction of total triangles that must be classified as supports
    /// before we consider the separation meaningful (avoids false positives on
    /// models with small decorative detachments).
    /// </summary>
    private const float MinSupportFraction = 0.01f;

    /// <summary>
    /// Vertex quantization grid size in mm. Vertices within this distance are
    /// treated as coincident, which handles floating-point rounding at shared edges.
    /// </summary>
    private const float VertexGridSize = 0.01f;

    private readonly struct QuantizedEdge : IEquatable<QuantizedEdge>
    {
        public readonly (int X, int Y, int Z) A;
        public readonly (int X, int Y, int Z) B;

        public QuantizedEdge((int X, int Y, int Z) p0, (int X, int Y, int Z) p1)
        {
            if (Compare(p0, p1) <= 0)
            {
                A = p0;
                B = p1;
            }
            else
            {
                A = p1;
                B = p0;
            }
        }

        public bool Equals(QuantizedEdge other) => A.Equals(other.A) && B.Equals(other.B);

        public override bool Equals(object? obj) => obj is QuantizedEdge other && Equals(other);

        public override int GetHashCode() => HashCode.Combine(A, B);

        private static int Compare((int X, int Y, int Z) left, (int X, int Y, int Z) right)
        {
            if (left.X != right.X) return left.X.CompareTo(right.X);
            if (left.Y != right.Y) return left.Y.CompareTo(right.Y);
            return left.Z.CompareTo(right.Z);
        }
    }

    /// <summary>
    /// Partitions the supplied triangles into model body and support geometry.
    /// Returns <c>null</c> for <c>supports</c> when no clear support components
    /// are found (e.g. single-component mesh or no component is small enough).
    /// </summary>
    public (List<Triangle3D> Model, List<Triangle3D>? Supports) Separate(List<Triangle3D> triangles)
    {
        if (triangles.Count == 0)
            return (triangles, null);

        // ── 1. Build union-find over triangle indices ─────────────────────────
        var parent = new int[triangles.Count];
        for (int i = 0; i < parent.Length; i++) parent[i] = i;

        // Maps quantized edge → index of the first triangle that used it.
        // Edge-based connectivity keeps point-contact tips detached from body.
        var edgeOwner = new Dictionary<QuantizedEdge, int>(triangles.Count * 3);

        for (int i = 0; i < triangles.Count; i++)
        {
            var tri = triangles[i];
            var q0 = Quantize(tri.V0);
            var q1 = Quantize(tri.V1);
            var q2 = Quantize(tri.V2);

            Union(parent, i, GetOrSetOwner(edgeOwner, new QuantizedEdge(q0, q1), i));
            Union(parent, i, GetOrSetOwner(edgeOwner, new QuantizedEdge(q1, q2), i));
            Union(parent, i, GetOrSetOwner(edgeOwner, new QuantizedEdge(q2, q0), i));
        }

        // ── 2. Compute component sizes ────────────────────────────────────────
        var componentSize = new Dictionary<int, int>();
        for (int i = 0; i < triangles.Count; i++)
        {
            var root = Find(parent, i);
            componentSize.TryGetValue(root, out var count);
            componentSize[root] = count + 1;
        }

        int maxSize = 0;
        foreach (var v in componentSize.Values)
            if (v > maxSize) maxSize = v;

        int sizeThreshold = (int)(maxSize * SupportSizeThreshold);

        // ── 3. Classify ───────────────────────────────────────────────────────
        var model = new List<Triangle3D>(triangles.Count);
        var supports = new List<Triangle3D>();

        for (int i = 0; i < triangles.Count; i++)
        {
            var root = Find(parent, i);
            if (componentSize[root] < sizeThreshold)
                supports.Add(triangles[i]);
            else
                model.Add(triangles[i]);
        }

        // Discard if the support fraction is negligible (likely not a supported model)
        if (supports.Count < triangles.Count * MinSupportFraction)
            return (triangles, null);

        return (model, supports);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private static (int X, int Y, int Z) Quantize(Vec3 v) =>
        ((int)MathF.Round(v.X / VertexGridSize),
         (int)MathF.Round(v.Y / VertexGridSize),
         (int)MathF.Round(v.Z / VertexGridSize));

    private static int GetOrSetOwner(Dictionary<QuantizedEdge, int> map, QuantizedEdge key, int idx)
    {
        if (!map.TryGetValue(key, out var owner))
        {
            map[key] = idx;
            return idx;
        }
        return owner;
    }

    private static int Find(int[] parent, int x)
    {
        while (parent[x] != x)
        {
            parent[x] = parent[parent[x]]; // path halving compression
            x = parent[x];
        }
        return x;
    }

    private static void Union(int[] parent, int a, int b)
    {
        a = Find(parent, a);
        b = Find(parent, b);
        if (a != b) parent[b] = a;
    }
}
