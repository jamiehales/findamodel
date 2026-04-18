# Slicing Algorithm Optimization Plan and Benchmark Results

## Hardware

- CPU: Intel Core i5-12400 (6 cores / 12 threads)
- GPU: NVIDIA GeForce RTX 3080 Ti (12GB VRAM)
- RAM: 32 GB DDR4
- OS: Windows 11 Pro

## Test Geometry

21 model placements from plate-export-repro-payload layout:
- 2 complex models (~4K triangles each, 32x64 sphere segments)
- 18 small models (~960 triangles each, 16x32 sphere segments)
- 1 medium model (~2.2K triangles, 24x48 sphere segments)
- Total: 27,424 triangles across 218.88mm x 122.88mm bed
- Target resolution: 3840x2400 (typical MSLA resin printer)

## Baseline Profiling Results (pre-optimization)

### Single Layer Render Times (ms)

| Height (mm) | Ortho CPU 960x600 | Ortho CPU 3840x2400 | Mesh CPU 960x600 | Mesh CPU 3840x2400 | GPU 3840x2400 |
|-------------|-------------------|----------------------|--------------------|--------------------|---------------|
| 0.025       | 20.5              | 322.8                | 19.2               | 344.9              | 126.0         |
| 0.500       | 20.8              | 323.7                | 20.8               | 332.9              | 120.8         |
| 1.000       | 38.8              | 380.6                | 31.2               | 306.5              | 118.2         |
| 2.000       | 24.6              | 354.1                | 20.1               | 346.8              | 113.2         |
| 5.000       | 23.1              | 360.6                | 18.9               | 377.4              | 102.3         |
| 10.000      | 26.8              | 287.9                | 17.7               | 309.4              | 98.8          |

### CPU Parallelism

| Resolution  | Wall time | CPU time | Effective parallelism |
|-------------|-----------|----------|-----------------------|
| 960x600     | 21.8ms    | 31.2ms   | 1.43x                 |
| 3840x2400   | 296.6ms   | 296.9ms  | 1.00x                 |

**Critical finding:** At 3840x2400, CPU parallelism is effectively 1.0x - the row-level
`Parallel.For` provides no benefit. With 2400 rows but only ~15-25% having triangle
candidates, most rows are trivially empty. The overhead of `Parallel.For` on 2400 items
with minimal per-item work exceeds the parallelism benefit.

### Batch Layer Rendering (960x600)

| Batch size | Wall time | Per layer | Parallelism |
|-----------|-----------|-----------|-------------|
| 1         | 19.4ms    | 19.4ms    | 0.80x       |
| 2         | 36.8ms    | 18.4ms    | 0.85x       |
| 4         | 37.5ms    | 9.4ms     | 3.75x       |
| 8         | 36.8ms    | 4.6ms     | 6.80x       |
| 16        | 84.1ms    | 5.3ms     | 6.50x       |
| 32        | 130.0ms   | 4.1ms     | 6.49x       |

Layer-level parallelism saturates at 6.5x (close to 6 physical cores). Good scaling.

### GPU Performance

| Metric             | Value     |
|--------------------|-----------|
| Backend            | nvidia-compute |
| 960x600 speedup    | 1.7x vs CPU |
| 3840x2400 speedup  | 2.9x vs CPU |
| GPU batch scaling   | None (51ms/layer regardless of batch size) |

GPU is currently disabled (`EnableGpuSliceProjection = false`).

### Memory Profile (32 layers batch, 3840x2400)

| Metric        | Value     |
|---------------|-----------|
| Base memory   | 19.4 MB   |
| Peak memory   | 320.4 MB  |
| Delta         | 301.1 MB  |
| Bitmap total  | 281.2 MB  |
| GC gen0       | 18        |
| GC gen1       | 14        |
| GC gen2       | 11        |

### Archive Generation (960x600, 400 layers)

| Method                  | Total time | Per layer | Memory delta |
|-------------------------|-----------|-----------|--------------|
| MeshIntersection        | 9.34s     | 23.3ms    | 273.7 MB     |
| OrthographicProjection  | 7.45s     | 18.6ms    | 245.4 MB     |

## Identified Hotspots and Optimization Opportunities

### 1. CRITICAL: Row-level parallelism ineffective at high resolution

The CPU orthographic path uses `Parallel.For(0, pixelHeight, row => ...)` but at
3840x2400, only ~25% of rows have triangle candidates. Even those rows process
very few triangles each. The Parallel.For overhead dominates.

**Optimization:** Partition non-empty rows into balanced chunks matching core count
and process chunks in parallel, with serial row iteration within each chunk. This avoids
the Parallel.For scheduling overhead per-row.

### 2. HIGH: Precomputed triangle data rebuilt per layer

`BuildPrecomputedTriangles()` and `BuildRowCandidates()` are called fresh for every
single layer render, even when the triangle set is the same across layers (common
in batch mode). Precomputation cost scales with triangle count.

**Optimization:** Cache precomputed triangles across layers in batch mode. Only
rebuild row candidates (which depend on slice height), reuse precomputed triangle
arrays.

### 3. HIGH: OrInto loop is naive byte-by-byte

The `OrInto()` method in PlateSliceRasterService iterates pixel-by-pixel to OR
bitmaps together. At 3840x2400 = 9.2M pixels, this is a hot loop.

**Optimization:** Use `Vector<byte>` (SIMD) for bulk OR operations, processing
16-32 bytes at a time.

### 4. MEDIUM: BuildRowCandidates allocates many small List<int>

Each non-empty row allocates a `List<int>` for candidate triangle indices.
At 2400 rows with ~600 active, this creates 600 small heap allocations per layer.

**Optimization:** Use a flat array with offset/count pairs instead of per-row lists.
Pre-count candidates, allocate a single int[] buffer, store (offset, count) per row.

### 5. MEDIUM: SliceBitmap cleanup clones entire pixel array multiple times

`RemoveUnsupportedHorizontalPixels()` calls `Pixels.Clone()`, then calls
`ClearUnsupportedRunInteriors()`, `RepairVerticalDropouts()` (with another clone),
`FillSmallInteriorVoids()`, `RepairThinInteriorHorizontalGaps()` (with clone per pass),
and `RemoveDetachedArtifacts()`. Each clone copies the full 9.2M pixel array.

**Optimization:** Reduce clones by combining passes or using a single scratch buffer.

### 6. MEDIUM: RemoveDetachedArtifacts uses flood-fill with Stack<int>

The connected component analysis allocates a `bool[]` visited array (9.2M) and uses
a `Stack<int>` for BFS. This adds GC pressure.

**Optimization:** Use ArrayPool for the visited buffer. Consider run-length-encoded
component tracking.

### 7. LOW: FillProjectedRow hit dedup is O(n^2)

The `TryAccumulateHit()` method scans all existing hits linearly to check for dedup.
With many triangles per row, this becomes quadratic.

**Optimization:** Unlikely to matter with typical hit counts (rarely >10 per row),
but could use a sorted insert path.

### 8. LOW: EncodePng allocates MemoryStream per layer

Each layer PNG encoding allocates a `MemoryStream` and creates an ImageSharp image.

**Optimization:** Pool MemoryStream buffers across layers.

## Implementation Plan

### Phase 1: CPU parallelism fix (biggest impact)
- Replace `Parallel.For(0, pixelHeight, ...)` with chunk-based parallelism
- Partition active (non-empty) rows into N chunks (N = processor count)
- Process chunks in parallel, rows serial within chunk
- Expected: 3-6x speedup on single-layer 3840x2400 (target <100ms)

### Phase 2: Precomputed triangle caching in batch mode
- Add precomputed triangle array caching to `RenderLayerBitmaps()`
- Compute once, reuse across all layers in batch
- Expected: ~10-15% speedup on batch rendering

### Phase 3: SIMD OrInto
- Use `Vector<byte>` for bulk OR operations in bitmap composition
- Expected: ~5-10x speedup for the OR phase (small % of total)

### Phase 4: Allocation reduction
- Flat buffer for row candidates instead of per-row List<int>
- ArrayPool for scratch buffers in cleanup methods
- Reduce pixel array clones in cleanup chain
- Expected: reduced GC pressure, fewer gen0/gen1 collections

### Phase 5: Cleanup method optimization
- Combine RepairVerticalDropouts passes into single pass
- Use pre-allocated scratch buffer instead of Pixels.Clone()
- Expected: faster cleanup phase, lower memory

---

## Benchmark Log

### Baseline (pre-optimization)

| Metric | Value |
|--------|-------|
| Single layer Ortho CPU 3840x2400 | 300-380ms |
| Batch 32 layers Ortho 960x600 | 130ms (4.1ms/layer) |
| Archive 400 layers 960x600 | 7.45s |
| Single layer GPU 3840x2400 | 99-126ms |
| CPU parallelism at 3840x2400 | 1.0x (broken) |
| Memory for 32 layers 3840x2400 | 301MB |
| GC gen0/gen1/gen2 | 18/14/11 |

### After Phase 1-5 (chunked parallelism, precomputed triangle caching, SIMD OrInto, ArrayPool)

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| Single layer Ortho CPU 3840x2400 | 300-380ms | 285-340ms | ~10% faster |
| Batch 32 layers 960x600 per-layer | 4.1ms | 4.3ms | Same |
| GC gen0 (32 batch 3840x2400) | 18 | 12 | -33% |
| GC gen1 (32 batch 3840x2400) | 14 | 6 | -57% |
| GC gen2 (32 batch 3840x2400) | 11 | 5 | -55% |

Changes applied:
- Phase 1: Replaced `Parallel.For(0, pixelHeight)` with `Parallel.ForEach(Partitioner.Create)` over active rows only
- Phase 2: Precomputed triangle caching across layers in batch mode (avoids redundant `BuildPrecomputedTriangles`)
- Phase 3: SIMD `Vector<byte>` OR in `OrInto` for bitmap composition
- Phase 4: `ArrayPool<bool>` for visited buffers in `FillSmallInteriorVoids` and `RemoveDetachedArtifacts`
- Phase 5: Reduced `Pixels.Clone()` overhead via shared buffers in cleanup chain
