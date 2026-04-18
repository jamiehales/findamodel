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

383/383 tests passed (8m 45s total), including:
- `CurrentPlate_Layer00011` MeshIntersection: 5m 19s (passed)
- `CurrentPlate_Layer00011` OrthographicProjection: 3m 22s (passed, previously crashed with OOM)
- All 5 new rotation tests: passed
