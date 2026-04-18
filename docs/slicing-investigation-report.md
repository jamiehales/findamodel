# Slicing Algorithm Investigation Report

## Summary

Investigation into two issues: (1) visual artifacts in sliced output related to rotation with high polygon counts, and (2) a major performance regression where only ~50% CPU and ~0.5% GPU utilization was observed during slicing.

Both issues have been resolved. All 383 backend tests pass, including the real-data `CurrentPlate_Layer00011` tests for both generators.

---

## Issue 1: Rotation-Related Artifacts

### Root Cause

When models are placed on a plate, Y-axis rotation is applied via `PlaceVertex`:

```csharp
new Vec3(v.X * cosA - v.Z * sinA + xMm, v.Y, v.X * sinA + v.Z * cosA + yMm)
```

Triangles sharing an edge have identical vertex positions before rotation. After rotation by arbitrary angles (e.g. 47.3 degrees), floating-point rounding causes these shared vertices to diverge slightly. The hit deduplication epsilon in both slice generators was too tight to account for this divergence.

### Affected Files

- `OrthographicProjectionSliceBitmapGenerator.cs` - Ray-casting (+X direction, Moller-Trumbore intersection)
- `MeshIntersectionSliceBitmapGenerator.cs` - Edge-plane intersection with scanline winding fill

### Fixes Applied

**1. Hit deduplication epsilon (both generators)**

The epsilon used to collapse nearly-identical intersection hits was increased from `0.0005f` (Orthographic) / `0.0001f` (MeshIntersection) to `0.002f`. This accommodates the floating-point spread introduced by trigonometric rotation while remaining well below the minimum pixel pitch (~0.05mm at typical print resolutions).

**2. Zero-delta hit filtering (both generators)**

Adjacent triangles can produce intersection hits whose winding contributions cancel (delta sum = 0). These no-op hits were left in the stream and could interfere with winding state tracking in edge cases. Added explicit filtering:

```csharp
// OrthographicProjectionSliceBitmapGenerator - FillWindingIntervals
if (hitDeltas[i] == 0) continue;

// MeshIntersectionSliceBitmapGenerator - FillScanline
if (deltaSum == 0) continue;
```

### New Tests (5)

All added to `SliceBitmapHarnessTests.cs`:

| Test | Description |
|------|-------------|
| `RotatedCubeSlice_ProducesSingleConnectedComponent` | A unit cube at 6 rotation angles - verifies single connected component (no artifact gaps) |
| `RotatedHighPolyCylinder_ProducesSingleConnectedComponent` | 256-segment cylinder at 4 angles - stress-tests high-polygon rotation dedup |
| `RotatedCubeSlice_StaysWithinRotatedBounds` | Verifies lit pixels stay within the analytical rotated bounding box |
| `RotatedSeparatedBoxes_RemainSeparateComponents` | Two separated boxes after rotation - verifies they remain distinct (no false merging) |
| `RotatedCubeSlice_MatchesAnalyticRotatedBitmap` | Compares rendered output against analytically computed rotated rectangle - max 2px deviation |

Helper methods added: `RotateTrianglesY()`, `AssertBitmapWithinBounds()`.

---

## Issue 2: Performance Regression

### Root Cause 1: Nested Parallelism (ThreadPool Starvation)

`RenderLayerBitmaps` (batch path) used `Parallel.For` across layers, and each layer's `RenderLayerBitmapCpu` used another `Parallel.For` across scanline rows. This created nested parallelism that flooded the ThreadPool, causing thread starvation and context-switch overhead.

### Fix

Added `RenderLayerBitmapCpuSerial` - a serial-row variant used when the outer batch loop already provides parallelism. The batch path now:
- 1-2 layers: processes sequentially (no parallel overhead)
- 3+ layers: `Parallel.For` across layers, each using serial row processing

### Root Cause 2: Sequential Group Processing

`RenderCompositeBatch` processed model groups (20+ in a typical plate) one-by-one in sequence, leaving most cores idle.

### Fix

Restructured `RenderCompositeBatch` with a smart parallelism strategy:

- **Multiple groups**: `Parallel.For` across groups (each group renders its layers serially)
- **Single group, non-batch generator**: `Parallel.For` across layers
- **Single group, batch generator**: Uses the batch API directly

### Root Cause 3: OOM from Unbounded Group Parallelism

The initial group parallelism fix had no memory budget, causing OOM when 20+ groups each allocated 16 high-resolution bitmaps simultaneously.

### Fix

Added a memory budget cap (512MB) that dynamically limits `MaxDegreeOfParallelism` based on per-group bitmap allocation:

```csharp
var estimatedBytesPerGroup = (long)resolutionX * resolutionY * batchLength;
var maxMemoryBudget = 512L * 1024 * 1024;
if (estimatedBytesPerGroup * maxGroupParallelism > maxMemoryBudget)
    maxGroupParallelism = Math.Max(2, (int)(maxMemoryBudget / estimatedBytesPerGroup));
```

### Other Changes

- `DefaultLayerBatchSize` increased from 8 to 16, better utilizing available cores for batch processing

---

## Files Modified

| File | Changes |
|------|---------|
| `backend/Services/OrthographicProjectionSliceBitmapGenerator.cs` | Epsilon increase, zero-delta filtering, serial row method, batch parallelism restructure |
| `backend/Services/MeshIntersectionSliceBitmapGenerator.cs` | Epsilon increase, zero-delta filtering |
| `backend/Services/PlateSliceRasterService.cs` | Batch size 8 to 16, group-level parallelism with memory cap, smart single/multi-group strategy |
| `backend.Tests/SliceBitmapHarnessTests.cs` | 5 new rotation tests, 2 helper methods |

## Test Results

393/393 tests passed (9m 27s total), including:
- `CurrentPlate_Layer00011` MeshIntersection: 5m 19s (passed)
- `CurrentPlate_Layer00011` OrthographicProjection: 3m 39s (passed)
- All 5 rotation tests and 8 horizontal-line artifact tests: passed

---

## Issue 3: Horizontal Line Bridging Artifact (Object 8)

### Root Cause

Both generators use a winding-number fill algorithm that occasionally produces a single solid row at a specific Z coordinate where the geometry has a thin connecting feature or coincident edge. Because `RemoveUnsupportedHorizontalPixels` treated each horizontal run as an atomic unit (keep or remove the entire run), a 252px solid run survived cleanup because its left edge overlapped with legitimately supported pixels - even though 150px of the run's interior had zero vertical support from either the row above or below.

### Fix Applied

Added `ClearUnsupportedRunInteriors` to `SliceBitmap.RemoveUnsupportedHorizontalPixels`, called after the initial run-level support check. This new pass scans each preserved run at pixel granularity and removes contiguous interior segments of 4 or more pixels that have no vertical support (no lit pixel directly above or below in the pre-modification reference bitmap).

**Effect on Object 8**: Row 3234 was reduced from 252px (single bridging run) to 99px (two separate runs matching the natural taper of adjacent rows).

### New Tests (8)

All added to `SliceBitmapHarnessTests.cs` and `PlateSliceRasterServiceTests.cs`:

| Test | Description |
|------|-------------|
| `SeparatedBoxes_NoBridgingArtifactAtAnyRow` | Two separated boxes - no row fills the gap (both generators) |
| `RotatedSeparatedBoxes_NoBridgingArtifactAtAnyRow` | Rotated separated boxes at 4 angles - no bridging artifacts |
| `UShapeModel_NoHorizontalBridgingAboveBase` | U-shape with base - slice above base shows two runs, not a bridge |
| `SliceBitmapCleanup_RemovesBridgingArtifactInSolidRow` | Synthetic artifact pattern - cleanup removes unsupported middle |
| `SliceBitmapCleanup_PreservesLegitimateWideSolidRun` | Wide solid run with full support - preserved correctly |

### Files Modified

| File | Changes |
|------|---------|
| `backend/Services/SliceBitmap.cs` | Added `ClearUnsupportedRunInteriors` method, called after initial run support check |
| `backend.Tests/SliceBitmapHarnessTests.cs` | 5 new bridging-artifact tests, `AssertNoHorizontalBridging` helper |

---

## Performance Optimizations (CPU + GPU)

### CPU Optimizations

Three optimizations applied to `OrthographicProjectionSliceBitmapGenerator`:

**1. Precomputed triangle data**

Added `PrecomputedTriangle` struct that precomputes per-triangle edge vectors (`edge1`, `edge2`), determinant (`a`), inverse determinant (`invA`), winding delta, and bounding box (`maxX`, `minZ`, `maxZ`). These values were previously recomputed for every ray-triangle intersection test. Now computed once per `BuildPrecomputedTriangles` call and reused across all rows.

**2. Ray-direction math simplification**

Since `RayDirection = (1, 0, 0)` is constant, several operations in the Moller-Trumbore intersection simplify:
- `h = cross((1,0,0), edge2) = (0, -edge2.Z, edge2.Y)` - eliminates cross product, uses two precomputed values
- `dot(s, h)` simplifies to `-s.Y * edge2.Z + s.Z * edge2.Y` - eliminates a dot product
- `dot(RayDirection, q)` simplifies to `q.X` - eliminates a dot product
- Winding delta derived from `a = -normalX` without extra cross product

**3. Removed per-ray GetWindingDelta**

Winding delta is now precomputed in `PrecomputedTriangle.WindingDelta`, eliminating a per-intersection cross product and conditional.

### GPU Optimizations

Three optimizations applied to `GlSliceProjectionContext`:

**1. Cached compute shader uniform locations**

`RenderWithCompute` previously called `gl.GetUniformLocation(computeProgram, ...)` for every uniform on every frame (17 lookups per layer). Now all 17 compute uniform locations are cached during shader compilation, matching the fragment shader path.

**2. Buffer.BlockCopy for FlipToBitmap**

Replaced `Array.Copy` with `System.Buffer.BlockCopy` in `FlipToBitmap` for byte-array row flipping during GPU readback.

**3. Shared readback buffer across batch layers**

The `rawPixels` byte array is now allocated once per batch and reused for each layer's `ReadPixels` call, instead of allocating a new array per layer.

### Benchmark Results

All benchmarks run on the same machine, 5 iterations each, warmup excluded.

| Configuration | Baseline (ms) | Optimized (ms) | Speedup |
|---|---|---|---|
| small-cube-180x180 (12 tris) | 6.85 | 4.29 | 1.6x |
| grid-480x480 (2352 tris) | 45.95 | 35.79 | 1.3x |
| grid-960x600 (2352 tris) | 93.61 | 82.42 | 1.1x |
| dense-grid-960x600 (19200 tris) | 98.09 | 91.70 | 1.1x |
| dense-grid-3840x2400 (19200 tris) | 1830.26 | 1368.79 | 1.3x |
| sphere-960x600 (3968 tris) | 90.37 | 77.90 | 1.2x |
| batch-16layers-960x600 (2352 tris) | 319.50 | 274.03 | 1.2x |

### Files Modified

| File | Changes |
|------|---------|
| `backend/Services/OrthographicProjectionSliceBitmapGenerator.cs` | PrecomputedTriangle struct, math simplification, per-ray winding removal |
| `backend/Services/GlSliceProjectionContext.cs` | Cached compute uniforms, Buffer.BlockCopy, shared readback buffer |
| `backend.Tests/SliceBenchmarkTests.cs` | New benchmark test file for CPU slice performance |

### Test Results

395/395 tests passed (9m 45s total), including all 34 SliceBitmapHarnessTests.
