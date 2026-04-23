using System.Diagnostics;
using System.Globalization;
using findamodel.Models;

namespace findamodel.Services;

public sealed class ModelRepairService(ILoggerFactory loggerFactory)
{
    public const string CurrentRepairVersion = "1";

    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Repair);

    public Task<ModelRepairResult> RepairForSlicingAsync(
        LoadedGeometry geometry,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        var sw = Stopwatch.StartNew();
        var diagnostics = new ModelRepairDiagnostics
        {
            InputTriangles = geometry.Triangles.Count,
            OptionsHash = options.ComputeDeterministicHash(),
        };

        if (!options.Enabled || geometry.Triangles.Count == 0)
        {
            diagnostics.OutputTriangles = geometry.Triangles.Count;
            diagnostics.DurationMs = sw.ElapsedMilliseconds;
            return Task.FromResult(new ModelRepairResult
            {
                Geometry = geometry,
                Diagnostics = diagnostics,
                UsedOriginalGeometry = true,
            });
        }

        try
        {
            var mesh = BuildMesh(geometry.Triangles);
            var eps = ComputeScaleAwareEpsilons(mesh.Vertices, options);
            diagnostics.AreaEpsilon = eps.AreaEpsilon;
            diagnostics.EdgeEpsilon = eps.EdgeEpsilon;
            diagnostics.WeldEpsilon = eps.WeldEpsilon;

            if (options.EnableDegenerateRemoval || options.EnableDuplicateRemoval)
                RemoveInvalidAndDuplicateTriangles(mesh, diagnostics, eps, options, ct);

            if (options.EnableVertexWeld)
                WeldVertices(mesh, diagnostics, eps, ct);

            var components = BuildComponentsAndOrient(mesh, diagnostics, ct);

            if (options.EnableDustComponentFiltering)
                RemoveDustComponents(mesh, components, diagnostics, options, ct);

            components = BuildComponentsAndOrient(mesh, diagnostics: null, ct);

            if (options.EnableHoleFill)
                FillSimpleBoundaryLoops(mesh, diagnostics, options, ct);

            components = BuildComponentsAndOrient(mesh, diagnostics: null, ct);

            if (options.EnableInternalVoidRepair)
                RepairInternalVoids(mesh, components, diagnostics, options, ct);

            components = BuildComponentsAndOrient(mesh, diagnostics: null, ct);

            if (options.EnableThinSlabDetection)
                DetectAndRemoveThinSlabPairs(mesh, components, diagnostics, options, ct);

            EvaluateManifoldAndIntersectionSeverity(mesh, diagnostics, ct);

            if (options.EnableFallbackRemesh)
                ApplyFallbackRepairIfNeeded(mesh, diagnostics, options, ct);

            var outputTriangles = mesh.ExportTriangles();
            var outputGeometry = RecomputeMetadata(outputTriangles);
            diagnostics.OutputTriangles = outputTriangles.Count;
            diagnostics.DurationMs = sw.ElapsedMilliseconds;

            logger.LogInformation(
                "Model repair complete: input={InputTriangles}, output={OutputTriangles}, removedDegenerate={RemovedDegenerateTriangles}, removedDuplicate={RemovedDuplicateTriangles}, flippedComponents={FlippedComponents}, removedDust={RemovedDustComponents}, cappedLoops={CappedBoundaryLoops}, voidsRemoved={VoidComponentsRemoved}, durationMs={DurationMs}",
                diagnostics.InputTriangles,
                diagnostics.OutputTriangles,
                diagnostics.RemovedDegenerateTriangles,
                diagnostics.RemovedDuplicateTriangles,
                diagnostics.FlippedComponents,
                diagnostics.RemovedDustComponents,
                diagnostics.CappedBoundaryLoops,
                diagnostics.VoidComponentsRemoved,
                diagnostics.DurationMs);

            return Task.FromResult(new ModelRepairResult
            {
                Geometry = outputGeometry,
                Diagnostics = diagnostics,
                UsedOriginalGeometry = false,
            });
        }
        catch (Exception ex)
        {
            diagnostics.DurationMs = sw.ElapsedMilliseconds;
            logger.LogWarning(ex, "Model repair failed after {DurationMs} ms", diagnostics.DurationMs);
            if (options.StrictMode)
                throw;

            diagnostics.OutputTriangles = geometry.Triangles.Count;
            return Task.FromResult(new ModelRepairResult
            {
                Geometry = geometry,
                Diagnostics = diagnostics,
                UsedOriginalGeometry = true,
            });
        }
    }

    private static LoadedGeometry RecomputeMetadata(List<Triangle3D> triangles)
    {
        if (triangles.Count == 0)
        {
            return new LoadedGeometry
            {
                Triangles = [],
                SphereCentre = new Vec3(0f, 0f, 0f),
                SphereRadius = 1f,
                DimensionXMm = 0f,
                DimensionYMm = 0f,
                DimensionZMm = 0f,
            };
        }

        var bounds = ComputeBounds(triangles.SelectMany(t => new[] { t.V0, t.V1, t.V2 }));
        var dimX = bounds.Max.X - bounds.Min.X;
        var dimY = bounds.Max.Y - bounds.Min.Y;
        var dimZ = bounds.Max.Z - bounds.Min.Z;
        var centre = new Vec3((bounds.Min.X + bounds.Max.X) * 0.5f, (bounds.Min.Y + bounds.Max.Y) * 0.5f, (bounds.Min.Z + bounds.Max.Z) * 0.5f);

        var radius = 0f;
        foreach (var triangle in triangles)
        {
            radius = MathF.Max(radius, Distance(centre, triangle.V0));
            radius = MathF.Max(radius, Distance(centre, triangle.V1));
            radius = MathF.Max(radius, Distance(centre, triangle.V2));
        }

        if (!float.IsFinite(radius) || radius < 1e-6f)
            radius = 1f;

        return new LoadedGeometry
        {
            Triangles = triangles,
            SphereCentre = centre,
            SphereRadius = radius,
            DimensionXMm = MathF.Max(0f, dimX),
            DimensionYMm = MathF.Max(0f, dimY),
            DimensionZMm = MathF.Max(0f, dimZ),
        };
    }

    private static void RemoveInvalidAndDuplicateTriangles(
        MutableMesh mesh,
        ModelRepairDiagnostics diagnostics,
        ScaleAwareEps eps,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        var kept = new List<MutableTriangle>(mesh.Triangles.Count);
        var seen = new HashSet<string>(StringComparer.Ordinal);

        foreach (var triangle in mesh.Triangles)
        {
            ct.ThrowIfCancellationRequested();

            if (!IsFinite(mesh.Vertices[triangle.I0]) || !IsFinite(mesh.Vertices[triangle.I1]) || !IsFinite(mesh.Vertices[triangle.I2]))
            {
                diagnostics.RemovedDegenerateTriangles++;
                continue;
            }

            var v0 = mesh.Vertices[triangle.I0];
            var v1 = mesh.Vertices[triangle.I1];
            var v2 = mesh.Vertices[triangle.I2];
            var e0 = Distance(v0, v1);
            var e1 = Distance(v1, v2);
            var e2 = Distance(v2, v0);

            var twiceArea = (v1 - v0).Cross(v2 - v0).Length;
            var area = 0.5f * twiceArea;

            if (options.EnableDegenerateRemoval && (area < eps.AreaEpsilon || e0 < eps.EdgeEpsilon || e1 < eps.EdgeEpsilon || e2 < eps.EdgeEpsilon))
            {
                diagnostics.RemovedDegenerateTriangles++;
                continue;
            }

            if (options.EnableDuplicateRemoval)
            {
                var key = BuildCanonicalTriangleKey(v0, v1, v2, eps.WeldEpsilon);
                if (!seen.Add(key))
                {
                    diagnostics.RemovedDuplicateTriangles++;
                    continue;
                }
            }

            kept.Add(triangle);
        }

        mesh.Triangles = kept;
    }

    private static void WeldVertices(MutableMesh mesh, ModelRepairDiagnostics diagnostics, ScaleAwareEps eps, CancellationToken ct)
    {
        var grid = new Dictionary<CellKey, List<int>>();
        var remap = new int[mesh.Vertices.Count];
        var welded = new List<Vec3>(mesh.Vertices.Count);

        for (var i = 0; i < mesh.Vertices.Count; i++)
        {
            ct.ThrowIfCancellationRequested();
            var vertex = mesh.Vertices[i];
            var baseCell = CellKey.From(vertex, eps.WeldEpsilon);

            var found = -1;
            foreach (var neighbor in baseCell.EnumerateNeighbors())
            {
                if (!grid.TryGetValue(neighbor, out var candidates))
                    continue;

                foreach (var candidate in candidates)
                {
                    if (Distance(vertex, welded[candidate]) <= eps.WeldEpsilon)
                    {
                        found = candidate;
                        break;
                    }
                }

                if (found >= 0)
                    break;
            }

            if (found >= 0)
            {
                remap[i] = found;
                diagnostics.WeldedVertexCount++;
                continue;
            }

            var newIndex = welded.Count;
            welded.Add(vertex);
            remap[i] = newIndex;

            if (!grid.TryGetValue(baseCell, out var list))
            {
                list = [];
                grid[baseCell] = list;
            }

            list.Add(newIndex);
        }

        var triangles = new List<MutableTriangle>(mesh.Triangles.Count);
        foreach (var triangle in mesh.Triangles)
        {
            var i0 = remap[triangle.I0];
            var i1 = remap[triangle.I1];
            var i2 = remap[triangle.I2];

            if (i0 == i1 || i1 == i2 || i2 == i0)
            {
                diagnostics.TrianglesCollapsedAfterWeld++;
                continue;
            }

            triangles.Add(new MutableTriangle(i0, i1, i2));
        }

        mesh.Vertices = welded;
        mesh.Triangles = triangles;
    }

    private static List<ComponentInfo> BuildComponentsAndOrient(
        MutableMesh mesh,
        ModelRepairDiagnostics? diagnostics,
        CancellationToken ct)
    {
        var adjacency = BuildTriangleAdjacency(mesh);
        var visited = new bool[mesh.Triangles.Count];
        var components = new List<ComponentInfo>();

        for (var i = 0; i < mesh.Triangles.Count; i++)
        {
            ct.ThrowIfCancellationRequested();
            if (visited[i])
                continue;

            var queue = new Queue<int>();
            queue.Enqueue(i);
            visited[i] = true;
            var triangles = new List<int>();

            while (queue.Count > 0)
            {
                ct.ThrowIfCancellationRequested();
                var current = queue.Dequeue();
                triangles.Add(current);

                foreach (var edge in adjacency[current])
                {
                    var neighbor = edge.NeighborTriangleIndex;
                    if (!visited[neighbor])
                    {
                        var currentTriangle = mesh.Triangles[current];
                        var neighborTriangle = mesh.Triangles[neighbor];

                        var currentDirected = currentTriangle.GetDirectedEdge(edge.CurrentEdgeSlot);
                        var neighborDirected = neighborTriangle.GetDirectedEdge(edge.NeighborEdgeSlot);

                        var sameDirection = currentDirected.Equals(neighborDirected);
                        if (sameDirection)
                            mesh.FlipTriangle(neighbor);

                        visited[neighbor] = true;
                        queue.Enqueue(neighbor);
                    }
                }
            }

            var signedVolume = ComputeComponentSignedVolume(mesh, triangles);
            if (signedVolume < 0)
            {
                foreach (var triangleIndex in triangles)
                    mesh.FlipTriangle(triangleIndex);

                if (diagnostics is not null)
                    diagnostics.FlippedComponents++;

                signedVolume = -signedVolume;
            }

            components.Add(new ComponentInfo(triangles, signedVolume));
        }

        return components;
    }

    private static void RemoveDustComponents(
        MutableMesh mesh,
        IReadOnlyList<ComponentInfo> components,
        ModelRepairDiagnostics diagnostics,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        if (components.Count <= 1)
            return;

        var keepComponent = components
            .Select((component, index) => new { component, index })
            .OrderByDescending(x => MathF.Abs(x.component.SignedVolume))
            .ThenByDescending(x => x.component.TriangleIndices.Count)
            .First()
            .index;

        var removeTriangles = new HashSet<int>();

        for (var componentIndex = 0; componentIndex < components.Count; componentIndex++)
        {
            ct.ThrowIfCancellationRequested();
            if (componentIndex == keepComponent)
                continue;

            var component = components[componentIndex];
            var bounds = ComputeComponentBounds(mesh, component.TriangleIndices);
            var diagonal = Distance(bounds.Min, bounds.Max);

            var isDust = component.TriangleIndices.Count < options.DustTriangleThreshold
                && diagonal < options.DustDiagonalThresholdMm;

            if (!isDust)
                continue;

            foreach (var triangleIndex in component.TriangleIndices)
                removeTriangles.Add(triangleIndex);

            diagnostics.RemovedDustComponents++;
        }

        if (removeTriangles.Count == 0)
            return;

        mesh.Triangles = mesh.Triangles
            .Where((_, idx) => !removeTriangles.Contains(idx))
            .ToList();
    }

    private static void FillSimpleBoundaryLoops(
        MutableMesh mesh,
        ModelRepairDiagnostics diagnostics,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        var edgeStats = BuildEdgeStats(mesh);
        var boundaryEdges = edgeStats.Where(kvp => kvp.Value.Count == 1).Select(kvp => kvp.Key).ToList();
        if (boundaryEdges.Count == 0)
            return;

        diagnostics.BoundaryLoopCount = boundaryEdges.Count;

        var adjacency = new Dictionary<int, List<int>>();
        foreach (var edge in boundaryEdges)
        {
            if (!adjacency.TryGetValue(edge.A, out var a))
            {
                a = [];
                adjacency[edge.A] = a;
            }

            if (!adjacency.TryGetValue(edge.B, out var b))
            {
                b = [];
                adjacency[edge.B] = b;
            }

            a.Add(edge.B);
            b.Add(edge.A);
        }

        var visitedPairs = new HashSet<(int, int)>();
        var capped = 0;

        foreach (var edge in boundaryEdges)
        {
            ct.ThrowIfCancellationRequested();
            if (capped >= options.MaxHoleLoopsToCap)
                break;

            if (visitedPairs.Contains((edge.A, edge.B)) || visitedPairs.Contains((edge.B, edge.A)))
                continue;

            var loop = ExtractLoop(edge.A, edge.B, adjacency, visitedPairs, options.MaxHoleLoopVertices);
            if (loop.Count < 3)
            {
                diagnostics.SkippedBoundaryLoops++;
                continue;
            }

            var loopBounds = ComputeBounds(loop.Select(i => mesh.Vertices[i]));
            var loopDiagonal = Distance(loopBounds.Min, loopBounds.Max);
            if (loopDiagonal > options.MaxHoleDiagonalMm)
            {
                diagnostics.SkippedBoundaryLoops++;
                continue;
            }

            if (!TryCapLoop(mesh, loop))
            {
                diagnostics.SkippedBoundaryLoops++;
                continue;
            }

            capped++;
            diagnostics.CappedBoundaryLoops++;
        }
    }

    private static void RepairInternalVoids(
        MutableMesh mesh,
        IReadOnlyList<ComponentInfo> components,
        ModelRepairDiagnostics diagnostics,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        if (components.Count < 2)
            return;

        var bounds = components
            .Select(component => ComputeComponentBounds(mesh, component.TriangleIndices))
            .ToList();

        var componentRemovals = new HashSet<int>();

        for (var i = 0; i < components.Count; i++)
        {
            ct.ThrowIfCancellationRequested();
            var component = components[i];
            var componentBounds = bounds[i];
            var centroid = ComputeComponentCentroid(mesh, component.TriangleIndices);

            var insideCandidate = false;
            for (var j = 0; j < components.Count; j++)
            {
                if (i == j)
                    continue;

                if (Contains(bounds[j], componentBounds))
                {
                    insideCandidate = true;
                    break;
                }
            }

            if (!insideCandidate)
                continue;

            var rayInside = IsPointInsideOtherComponents(mesh, components, i, centroid, options.InternalVoidRayCount);
            if (!rayInside)
                continue;

            diagnostics.InternalVoidComponentsDetected++;
            var isInverted = ComputeComponentSignedVolume(mesh, component.TriangleIndices) < 0;

            if (isInverted)
            {
                foreach (var triangleIndex in component.TriangleIndices)
                    mesh.FlipTriangle(triangleIndex);

                diagnostics.InvertedShellsFlipped++;
                continue;
            }

            if (MathF.Abs(component.SignedVolume) < options.MinVoidVolumeMm3)
            {
                componentRemovals.Add(i);
                diagnostics.VoidComponentsRemoved++;
            }
        }

        if (componentRemovals.Count == 0)
            return;

        var trianglesToRemove = new HashSet<int>();
        foreach (var componentIndex in componentRemovals)
        {
            foreach (var triangleIndex in components[componentIndex].TriangleIndices)
                trianglesToRemove.Add(triangleIndex);
        }

        mesh.Triangles = mesh.Triangles
            .Where((_, idx) => !trianglesToRemove.Contains(idx))
            .ToList();
    }

    private static void DetectAndRemoveThinSlabPairs(
        MutableMesh mesh,
        IReadOnlyList<ComponentInfo> components,
        ModelRepairDiagnostics diagnostics,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        if (components.Count < 2)
            return;

        var bounds = components
            .Select(component => ComputeComponentBounds(mesh, component.TriangleIndices))
            .ToArray();

        var removeTriangles = new HashSet<int>();

        for (var i = 0; i < components.Count; i++)
        {
            ct.ThrowIfCancellationRequested();
            for (var j = i + 1; j < components.Count; j++)
            {
                var overlap = ComputeAabbOverlapRatio(bounds[i], bounds[j]);
                if (overlap < options.ThinSlabAabbOverlapThreshold)
                    continue;

                var minDistance = ComputeApproximateComponentDistance(mesh, components[i], components[j]);
                if (minDistance > options.MinWallThicknessMm)
                    continue;

                diagnostics.ThinSlabPairsDetected++;

                var volumeI = MathF.Abs(components[i].SignedVolume);
                var volumeJ = MathF.Abs(components[j].SignedVolume);
                var removeComponent = volumeI <= volumeJ ? i : j;
                foreach (var triangleIndex in components[removeComponent].TriangleIndices)
                    removeTriangles.Add(triangleIndex);

                diagnostics.ThinSlabPairsRemoved++;
            }
        }

        if (removeTriangles.Count > 0)
        {
            mesh.Triangles = mesh.Triangles
                .Where((_, idx) => !removeTriangles.Contains(idx))
                .ToList();
        }
    }

    private static void EvaluateManifoldAndIntersectionSeverity(MutableMesh mesh, ModelRepairDiagnostics diagnostics, CancellationToken ct)
    {
        var edgeStats = BuildEdgeStats(mesh);
        diagnostics.NonManifoldEdgeCount = edgeStats.Count(kvp => kvp.Value.Count > 2);

        if (mesh.Triangles.Count > 3000)
        {
            diagnostics.SelfIntersectionEstimateCount = 0;
            return;
        }

        var count = 0;
        for (var i = 0; i < mesh.Triangles.Count; i++)
        {
            ct.ThrowIfCancellationRequested();
            var a = mesh.Triangles[i];
            var boundsA = ComputeBounds(GetTriangleVertices(mesh, a));

            for (var j = i + 1; j < mesh.Triangles.Count; j++)
            {
                var b = mesh.Triangles[j];
                if (SharesVertex(a, b))
                    continue;

                var boundsB = ComputeBounds(GetTriangleVertices(mesh, b));
                if (!Overlaps(boundsA, boundsB))
                    continue;

                if (TriangleTriangleLikelyIntersects(mesh, a, b))
                    count++;
            }
        }

        diagnostics.SelfIntersectionEstimateCount = count;
    }

    private static void ApplyFallbackRepairIfNeeded(
        MutableMesh mesh,
        ModelRepairDiagnostics diagnostics,
        ModelRepairOptions options,
        CancellationToken ct)
    {
        if (diagnostics.NonManifoldEdgeCount < options.NonManifoldEdgeFallbackThreshold
            && diagnostics.SelfIntersectionEstimateCount < options.SelfIntersectionFallbackThreshold)
        {
            return;
        }

        var components = BuildComponentsAndOrient(mesh, diagnostics: null, ct);
        if (components.Count == 0)
            return;

        var keep = components
            .OrderByDescending(component => MathF.Abs(component.SignedVolume))
            .ThenByDescending(component => component.TriangleIndices.Count)
            .First();

        var keepSet = keep.TriangleIndices.ToHashSet();
        mesh.Triangles = mesh.Triangles
            .Where((_, idx) => keepSet.Contains(idx))
            .ToList();

        diagnostics.UsedFallbackRemesh = true;
    }

    private static MutableMesh BuildMesh(IReadOnlyList<Triangle3D> triangles)
    {
        var vertices = new List<Vec3>(triangles.Count * 3);
        var meshTriangles = new List<MutableTriangle>(triangles.Count);

        foreach (var triangle in triangles)
        {
            var i0 = vertices.Count;
            vertices.Add(triangle.V0);
            var i1 = vertices.Count;
            vertices.Add(triangle.V1);
            var i2 = vertices.Count;
            vertices.Add(triangle.V2);
            meshTriangles.Add(new MutableTriangle(i0, i1, i2));
        }

        return new MutableMesh(vertices, meshTriangles);
    }

    private static IReadOnlyDictionary<EdgeKey, List<(int TriangleIndex, int EdgeSlot)>> BuildEdgeStats(MutableMesh mesh)
    {
        var edges = new Dictionary<EdgeKey, List<(int TriangleIndex, int EdgeSlot)>>();
        for (var i = 0; i < mesh.Triangles.Count; i++)
        {
            var triangle = mesh.Triangles[i];
            var e0 = EdgeKey.CreateUnordered(triangle.I0, triangle.I1);
            var e1 = EdgeKey.CreateUnordered(triangle.I1, triangle.I2);
            var e2 = EdgeKey.CreateUnordered(triangle.I2, triangle.I0);

            if (!edges.TryGetValue(e0, out var l0)) edges[e0] = l0 = [];
            if (!edges.TryGetValue(e1, out var l1)) edges[e1] = l1 = [];
            if (!edges.TryGetValue(e2, out var l2)) edges[e2] = l2 = [];
            l0.Add((i, 0));
            l1.Add((i, 1));
            l2.Add((i, 2));
        }

        return edges;
    }

    private static List<List<AdjacencyEdge>> BuildTriangleAdjacency(MutableMesh mesh)
    {
        var edgeStats = BuildEdgeStats(mesh);
        var adjacency = Enumerable.Range(0, mesh.Triangles.Count).Select(_ => new List<AdjacencyEdge>()).ToList();

        foreach (var stats in edgeStats.Values)
        {
            if (stats.Count != 2)
                continue;

            var a = stats[0];
            var b = stats[1];
            adjacency[a.TriangleIndex].Add(new AdjacencyEdge(b.TriangleIndex, a.EdgeSlot, b.EdgeSlot));
            adjacency[b.TriangleIndex].Add(new AdjacencyEdge(a.TriangleIndex, b.EdgeSlot, a.EdgeSlot));
        }

        return adjacency;
    }

    private static bool TryCapLoop(MutableMesh mesh, IReadOnlyList<int> loop)
    {
        if (loop.Count < 3)
            return false;

        var centroid = new Vec3(0f, 0f, 0f);
        foreach (var vertexIndex in loop)
            centroid += mesh.Vertices[vertexIndex];
        centroid *= 1f / loop.Count;

        var centroidIndex = mesh.Vertices.Count;
        mesh.Vertices.Add(centroid);

        for (var i = 0; i < loop.Count; i++)
        {
            var a = loop[i];
            var b = loop[(i + 1) % loop.Count];
            if (a == b || a == centroidIndex || b == centroidIndex)
                continue;

            mesh.Triangles.Add(new MutableTriangle(a, b, centroidIndex));
        }

        return true;
    }

    private static List<int> ExtractLoop(
        int start,
        int next,
        IReadOnlyDictionary<int, List<int>> adjacency,
        ISet<(int, int)> visitedPairs,
        int maxVertices)
    {
        var loop = new List<int> { start };
        var previous = start;
        var current = next;

        while (loop.Count <= maxVertices)
        {
            loop.Add(current);
            visitedPairs.Add((previous, current));
            visitedPairs.Add((current, previous));

            if (!adjacency.TryGetValue(current, out var neighbors) || neighbors.Count == 0)
                break;

            var hasCandidate = false;
            var candidate = 0;
            foreach (var neighbor in neighbors)
            {
                if (neighbor == previous)
                    continue;

                candidate = neighbor;
                hasCandidate = true;
                break;
            }

            if (!hasCandidate)
                break;

            previous = current;
            current = candidate;

            if (current == start)
            {
                loop.RemoveAt(loop.Count - 1);
                return loop;
            }
        }

        return [];
    }

    private static float ComputeAabbOverlapRatio(Bounds a, Bounds b)
    {
        var x = MathF.Max(0f, MathF.Min(a.Max.X, b.Max.X) - MathF.Max(a.Min.X, b.Min.X));
        var y = MathF.Max(0f, MathF.Min(a.Max.Y, b.Max.Y) - MathF.Max(a.Min.Y, b.Min.Y));
        var z = MathF.Max(0f, MathF.Min(a.Max.Z, b.Max.Z) - MathF.Max(a.Min.Z, b.Min.Z));

        var overlapVolume = x * y * z;
        if (overlapVolume <= 0f)
            return 0f;

        var aVolume = MathF.Max(1e-8f, (a.Max.X - a.Min.X) * (a.Max.Y - a.Min.Y) * (a.Max.Z - a.Min.Z));
        var bVolume = MathF.Max(1e-8f, (b.Max.X - b.Min.X) * (b.Max.Y - b.Min.Y) * (b.Max.Z - b.Min.Z));
        var minVolume = MathF.Min(aVolume, bVolume);
        return overlapVolume / minVolume;
    }

    private static float ComputeApproximateComponentDistance(MutableMesh mesh, ComponentInfo a, ComponentInfo b)
    {
        static IEnumerable<Vec3> SampleVertices(MutableMesh mesh, IReadOnlyList<int> triangleIndices)
        {
            var emitted = new HashSet<int>();
            foreach (var triangleIndex in triangleIndices)
            {
                var triangle = mesh.Triangles[triangleIndex];
                if (emitted.Add(triangle.I0)) yield return mesh.Vertices[triangle.I0];
                if (emitted.Add(triangle.I1)) yield return mesh.Vertices[triangle.I1];
                if (emitted.Add(triangle.I2)) yield return mesh.Vertices[triangle.I2];
                if (emitted.Count > 256) yield break;
            }
        }

        var sampledA = SampleVertices(mesh, a.TriangleIndices).ToArray();
        var sampledB = SampleVertices(mesh, b.TriangleIndices).ToArray();

        var min = float.MaxValue;
        foreach (var va in sampledA)
        {
            foreach (var vb in sampledB)
                min = MathF.Min(min, Distance(va, vb));
        }

        return min;
    }

    private static bool IsPointInsideOtherComponents(
        MutableMesh mesh,
        IReadOnlyList<ComponentInfo> components,
        int componentIndex,
        Vec3 point,
        int rayCount)
    {
        var hitVotes = 0;
        var directions = BuildFibonacciDirections(Math.Max(8, rayCount));

        foreach (var direction in directions)
        {
            var intersections = 0;
            for (var component = 0; component < components.Count; component++)
            {
                if (component == componentIndex)
                    continue;

                foreach (var triangleIndex in components[component].TriangleIndices)
                {
                    var triangle = mesh.Triangles[triangleIndex];
                    var v0 = mesh.Vertices[triangle.I0];
                    var v1 = mesh.Vertices[triangle.I1];
                    var v2 = mesh.Vertices[triangle.I2];
                    if (RayIntersectsTriangle(point, direction, v0, v1, v2))
                        intersections++;
                }
            }

            if ((intersections & 1) == 1)
                hitVotes++;
        }

        return hitVotes > (directions.Length / 2);
    }

    private static Vec3[] BuildFibonacciDirections(int count)
    {
        var directions = new Vec3[count];
        var golden = (1f + MathF.Sqrt(5f)) * 0.5f;

        for (var i = 0; i < count; i++)
        {
            var t = (i + 0.5f) / count;
            var y = 1f - (2f * t);
            var r = MathF.Sqrt(MathF.Max(0f, 1f - (y * y)));
            var theta = 2f * MathF.PI * i / golden;
            directions[i] = new Vec3(r * MathF.Cos(theta), y, r * MathF.Sin(theta));
        }

        return directions;
    }

    private static bool RayIntersectsTriangle(Vec3 origin, Vec3 direction, Vec3 v0, Vec3 v1, Vec3 v2)
    {
        var eps = 1e-6f;
        var edge1 = v1 - v0;
        var edge2 = v2 - v0;
        var pvec = direction.Cross(edge2);
        var det = edge1.Dot(pvec);

        if (MathF.Abs(det) < eps)
            return false;

        var invDet = 1f / det;
        var tvec = origin - v0;
        var u = tvec.Dot(pvec) * invDet;
        if (u < 0f || u > 1f)
            return false;

        var qvec = tvec.Cross(edge1);
        var v = direction.Dot(qvec) * invDet;
        if (v < 0f || u + v > 1f)
            return false;

        var t = edge2.Dot(qvec) * invDet;
        return t > eps;
    }

    private static float ComputeComponentSignedVolume(MutableMesh mesh, IReadOnlyList<int> triangleIndices)
    {
        var volume = 0d;
        foreach (var triangleIndex in triangleIndices)
        {
            var triangle = mesh.Triangles[triangleIndex];
            var v0 = mesh.Vertices[triangle.I0];
            var v1 = mesh.Vertices[triangle.I1];
            var v2 = mesh.Vertices[triangle.I2];
            volume += SignedTetrahedronVolume(v0, v1, v2);
        }

        return (float)volume;
    }

    private static double SignedTetrahedronVolume(Vec3 a, Vec3 b, Vec3 c)
    {
        return (a.X * ((double)b.Y * c.Z - (double)b.Z * c.Y)
            - a.Y * ((double)b.X * c.Z - (double)b.Z * c.X)
            + a.Z * ((double)b.X * c.Y - (double)b.Y * c.X)) / 6d;
    }

    private static Vec3 ComputeComponentCentroid(MutableMesh mesh, IReadOnlyList<int> triangleIndices)
    {
        var sum = new Vec3(0f, 0f, 0f);
        var count = 0;
        foreach (var triangleIndex in triangleIndices)
        {
            var triangle = mesh.Triangles[triangleIndex];
            sum += mesh.Vertices[triangle.I0];
            sum += mesh.Vertices[triangle.I1];
            sum += mesh.Vertices[triangle.I2];
            count += 3;
        }

        if (count == 0)
            return new Vec3(0f, 0f, 0f);

        return sum * (1f / count);
    }

    private static Bounds ComputeComponentBounds(MutableMesh mesh, IReadOnlyList<int> triangleIndices)
    {
        return ComputeBounds(triangleIndices.SelectMany(idx =>
        {
            var t = mesh.Triangles[idx];
            return new[] { mesh.Vertices[t.I0], mesh.Vertices[t.I1], mesh.Vertices[t.I2] };
        }));
    }

    private static Bounds ComputeBounds(IEnumerable<Vec3> vertices)
    {
        var hasAny = false;
        var min = new Vec3(float.MaxValue, float.MaxValue, float.MaxValue);
        var max = new Vec3(float.MinValue, float.MinValue, float.MinValue);

        foreach (var vertex in vertices)
        {
            hasAny = true;
            min = new Vec3(MathF.Min(min.X, vertex.X), MathF.Min(min.Y, vertex.Y), MathF.Min(min.Z, vertex.Z));
            max = new Vec3(MathF.Max(max.X, vertex.X), MathF.Max(max.Y, vertex.Y), MathF.Max(max.Z, vertex.Z));
        }

        if (!hasAny)
            return new Bounds(new Vec3(0f, 0f, 0f), new Vec3(0f, 0f, 0f));

        return new Bounds(min, max);
    }

    private static ScaleAwareEps ComputeScaleAwareEpsilons(IReadOnlyList<Vec3> vertices, ModelRepairOptions options)
    {
        var bounds = ComputeBounds(vertices);
        var d = Distance(bounds.Min, bounds.Max);
        if (d <= 0f || !float.IsFinite(d))
            d = 1f;

        var area = Clamp(d * d * 1e-12f * options.AreaEpsilonMultiplier, 1e-12f, 1e-4f);
        var edge = Clamp(d * 1e-7f * options.EdgeEpsilonMultiplier, 1e-6f, 1e-2f);
        var weld = Clamp(d * 5e-7f * options.WeldEpsilonMultiplier, 1e-5f, 5e-2f);

        return new ScaleAwareEps(area, edge, weld);
    }

    private static IEnumerable<Vec3> GetTriangleVertices(MutableMesh mesh, MutableTriangle triangle)
    {
        yield return mesh.Vertices[triangle.I0];
        yield return mesh.Vertices[triangle.I1];
        yield return mesh.Vertices[triangle.I2];
    }

    private static bool TriangleTriangleLikelyIntersects(MutableMesh mesh, MutableTriangle a, MutableTriangle b)
    {
        foreach (var edge in a.GetSegments())
        {
            if (SegmentIntersectsTriangle(mesh.Vertices[edge.Item1], mesh.Vertices[edge.Item2], mesh.Vertices[b.I0], mesh.Vertices[b.I1], mesh.Vertices[b.I2]))
                return true;
        }

        foreach (var edge in b.GetSegments())
        {
            if (SegmentIntersectsTriangle(mesh.Vertices[edge.Item1], mesh.Vertices[edge.Item2], mesh.Vertices[a.I0], mesh.Vertices[a.I1], mesh.Vertices[a.I2]))
                return true;
        }

        return false;
    }

    private static bool SegmentIntersectsTriangle(Vec3 s0, Vec3 s1, Vec3 v0, Vec3 v1, Vec3 v2)
    {
        var direction = s1 - s0;
        if (!RayIntersectsTriangle(s0, direction, v0, v1, v2))
            return false;

        var length = direction.Length;
        return length > 1e-7f;
    }

    private static bool SharesVertex(MutableTriangle a, MutableTriangle b)
    {
        return a.I0 == b.I0 || a.I0 == b.I1 || a.I0 == b.I2
            || a.I1 == b.I0 || a.I1 == b.I1 || a.I1 == b.I2
            || a.I2 == b.I0 || a.I2 == b.I1 || a.I2 == b.I2;
    }

    private static bool Contains(Bounds outer, Bounds inner)
    {
        return outer.Min.X <= inner.Min.X && outer.Min.Y <= inner.Min.Y && outer.Min.Z <= inner.Min.Z
            && outer.Max.X >= inner.Max.X && outer.Max.Y >= inner.Max.Y && outer.Max.Z >= inner.Max.Z;
    }

    private static bool Overlaps(Bounds a, Bounds b)
    {
        return a.Min.X <= b.Max.X && a.Max.X >= b.Min.X
            && a.Min.Y <= b.Max.Y && a.Max.Y >= b.Min.Y
            && a.Min.Z <= b.Max.Z && a.Max.Z >= b.Min.Z;
    }

    private static string BuildCanonicalTriangleKey(Vec3 v0, Vec3 v1, Vec3 v2, float eps)
    {
        static string Quantize(Vec3 v, float epsilon)
        {
            var qx = MathF.Round(v.X / epsilon);
            var qy = MathF.Round(v.Y / epsilon);
            var qz = MathF.Round(v.Z / epsilon);
            return string.Create(CultureInfo.InvariantCulture, $"{qx:G9}:{qy:G9}:{qz:G9}");
        }

        var ordered = new[]
        {
            Quantize(v0, eps),
            Quantize(v1, eps),
            Quantize(v2, eps),
        };
        Array.Sort(ordered, StringComparer.Ordinal);
        return string.Join("|", ordered);
    }

    private static bool IsFinite(Vec3 v) => float.IsFinite(v.X) && float.IsFinite(v.Y) && float.IsFinite(v.Z);

    private static float Clamp(float value, float min, float max) => MathF.Max(min, MathF.Min(max, value));

    private static float Distance(Vec3 a, Vec3 b)
    {
        var dx = a.X - b.X;
        var dy = a.Y - b.Y;
        var dz = a.Z - b.Z;
        return MathF.Sqrt((dx * dx) + (dy * dy) + (dz * dz));
    }

    private readonly record struct ScaleAwareEps(float AreaEpsilon, float EdgeEpsilon, float WeldEpsilon);

    private readonly record struct ComponentInfo(IReadOnlyList<int> TriangleIndices, float SignedVolume);

    private readonly record struct Bounds(Vec3 Min, Vec3 Max);

    private readonly record struct AdjacencyEdge(int NeighborTriangleIndex, int CurrentEdgeSlot, int NeighborEdgeSlot);

    private readonly record struct CellKey(int X, int Y, int Z)
    {
        public static CellKey From(Vec3 v, float epsilon)
            => new(
                (int)MathF.Floor(v.X / epsilon),
                (int)MathF.Floor(v.Y / epsilon),
                (int)MathF.Floor(v.Z / epsilon));

        public IEnumerable<CellKey> EnumerateNeighbors()
        {
            for (var dx = -1; dx <= 1; dx++)
            {
                for (var dy = -1; dy <= 1; dy++)
                {
                    for (var dz = -1; dz <= 1; dz++)
                        yield return new CellKey(X + dx, Y + dy, Z + dz);
                }
            }
        }
    }

    private readonly record struct EdgeKey(int A, int B)
    {
        public static EdgeKey CreateUnordered(int a, int b) => a <= b ? new EdgeKey(a, b) : new EdgeKey(b, a);
    }

    private sealed class MutableMesh(List<Vec3> vertices, List<MutableTriangle> triangles)
    {
        public List<Vec3> Vertices { get; set; } = vertices;
        public List<MutableTriangle> Triangles { get; set; } = triangles;

        public void FlipTriangle(int triangleIndex)
        {
            var t = Triangles[triangleIndex];
            Triangles[triangleIndex] = new MutableTriangle(t.I0, t.I2, t.I1);
        }

        public List<Triangle3D> ExportTriangles()
        {
            var result = new List<Triangle3D>(Triangles.Count);
            foreach (var triangle in Triangles)
            {
                var v0 = Vertices[triangle.I0];
                var v1 = Vertices[triangle.I1];
                var v2 = Vertices[triangle.I2];
                var normal = (v1 - v0).Cross(v2 - v0).Normalized;
                result.Add(new Triangle3D(v0, v1, v2, normal));
            }

            return result;
        }
    }

    private readonly record struct MutableTriangle(int I0, int I1, int I2)
    {
        public (int, int) GetDirectedEdge(int edgeSlot) => edgeSlot switch
        {
            0 => (I0, I1),
            1 => (I1, I2),
            2 => (I2, I0),
            _ => throw new ArgumentOutOfRangeException(nameof(edgeSlot)),
        };

        public IEnumerable<(int, int)> GetSegments()
        {
            yield return (I0, I1);
            yield return (I1, I2);
            yield return (I2, I0);
        }
    }
}
