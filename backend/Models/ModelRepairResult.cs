using findamodel.Services;

namespace findamodel.Models;

public sealed class ModelRepairDiagnostics
{
    public int InputTriangles { get; set; }
    public int OutputTriangles { get; set; }

    public int RemovedDegenerateTriangles { get; set; }
    public int RemovedDuplicateTriangles { get; set; }
    public int WeldedVertexCount { get; set; }
    public int TrianglesCollapsedAfterWeld { get; set; }

    public int FlippedComponents { get; set; }
    public int RemovedDustComponents { get; set; }
    public int BoundaryLoopCount { get; set; }
    public int CappedBoundaryLoops { get; set; }
    public int SkippedBoundaryLoops { get; set; }

    public int InternalVoidComponentsDetected { get; set; }
    public int InvertedShellsFlipped { get; set; }
    public int VoidComponentsRemoved { get; set; }

    public int ThinSlabPairsDetected { get; set; }
    public int ThinSlabPairsRemoved { get; set; }

    public int NonManifoldEdgeCount { get; set; }
    public int SelfIntersectionEstimateCount { get; set; }
    public bool UsedFallbackRemesh { get; set; }

    public float AreaEpsilon { get; set; }
    public float EdgeEpsilon { get; set; }
    public float WeldEpsilon { get; set; }

    public long DurationMs { get; set; }
    public string OptionsHash { get; set; } = string.Empty;
}

public sealed class ModelRepairResult
{
    public required LoadedGeometry Geometry { get; init; }
    public required ModelRepairDiagnostics Diagnostics { get; init; }
    public bool UsedOriginalGeometry { get; init; }
}
