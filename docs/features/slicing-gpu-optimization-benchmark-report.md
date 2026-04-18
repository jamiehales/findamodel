# GPU slicing optimization benchmark report

Date: 2026-04-16

## Scope

This report records the optimization work completed for the GPU-backed slicing roadmap and the benchmark evidence gathered after each phase. Phases 1 through 3 now cover the full roadmap within the current portable OpenGL implementation.

## Baseline

Environment:
- Benchmark source: backend.Tests slice performance tests
- Configuration: orthographic GPU path enabled when GL is available
- Notes: timings below are the pre-optimization baseline captured before the new changes in this task

### Baseline timings

| Case | Method | Time ms | Notes |
| --- | --- | ---: | --- |
| 1M triangles | MeshIntersection | 438.78 | Non-empty slice |
| 1M triangles | OrthographicProjection | 439.80 | CPU path |
| 1M triangles | OrthographicProjection GPU | 486.13 | Existing GPU path |
| 1M triangles benchmark | MeshIntersection | 601.66 | 48x48 synthetic benchmark |
| 1M triangles benchmark | OrthographicProjection | 427.06 | 48x48 synthetic benchmark |
| 5M triangles benchmark | MeshIntersection | 2162.42 | 48x48 synthetic benchmark |
| 5M triangles benchmark | OrthographicProjection | 2125.63 | 48x48 synthetic benchmark |
| 10M triangles benchmark | MeshIntersection | 4122.01 | 48x48 synthetic benchmark |
| 10M triangles benchmark | OrthographicProjection | 5184.11 | 48x48 synthetic benchmark |
| 100M triangles benchmark | MeshIntersection | 42918.01 | 48x48 synthetic benchmark |
| 100M triangles benchmark | OrthographicProjection | 43244.91 | 48x48 synthetic benchmark |

## Optimization log

### Optimization phase 1

Implemented items from the roadmap:
1. Persist geometry on the GPU
2. Add a GPU spatial index
4. Batch multiple layers per dispatch group
5. Avoid unnecessary full texture readback
6. Parallelize PNG encoding with GPU rendering
7. Pre-filter triangles by layer range on CPU before GPU submission
8. Use packed data formats
9. Add vendor-specific fast paths
10. Add automated GPU profiling and regression benchmarks

Code added in this phase:
- GPU-resident cached triangle, bounds, and row-bin textures in GlSliceProjectionContext
- Row-group spatial indexing by projected Z bands
- Single-channel R8 framebuffer readback instead of full RGBA readback
- Packed RGBA16F geometry uploads
- Batched layer rendering through RenderLayerBitmaps and TryRenderBatch
- Producer-consumer style PNG encoding overlap in PlateSliceRasterService
- NVIDIA-tuned row grouping for the GPU path
- End-to-end PNG archive throughput benchmark in the test suite

### Phase 1 benchmark snapshot

| Case | Baseline ms | After phase 1 ms | Change |
| --- | ---: | ---: | ---: |
| 1M orthographic benchmark | 427.06 | 458.08 | -7.3% |
| 5M orthographic benchmark | 2125.63 | 2310.67 | -8.7% |
| 10M orthographic benchmark | 5184.11 | 4595.90 | +11.3% |
| 100M orthographic benchmark | 43244.91 | 44254.63 | -2.3% |
| Orthographic archive throughput | n/a | 2925.13 | new benchmark |
| Mesh archive throughput | n/a | 6726.32 | new benchmark |

### Optimization phase 2

Implemented the remaining hot-path cleanup and batching polish:
3. Move the raster logic toward a compute-style batched dispatch model within the current portable OpenGL path

Code added in this phase:
- Direct single-layer GPU filtering without unnecessary batch de-duplication
- Lower-overhead sampled geometry signatures for GPU cache reuse
- Final tuning of the batch path for real export jobs

### Phase 2 benchmark snapshot

| Case | After phase 1 ms | Final ms | Change |
| --- | ---: | ---: | ---: |
| 1M orthographic GPU compare | 599.00 | 509.16 | +15.0% |
| 1M orthographic benchmark | 458.08 | 455.92 | +0.5% |
| 10M orthographic benchmark | 4595.90 | 4369.12 | +4.9% |
| Orthographic archive throughput | 2925.13 | 3182.30 | -8.8% |

### Optimization phase 3

Implemented the remaining roadmap items in the current stack:
- A fuller GPU uniform-grid spatial index across X and Z tile bins
- A compute shader backend that replaces the fragment evaluation path when OpenGL 4.3 or later is available
- An NVIDIA-specific fast path using larger compute workgroups and denser tile indexing on supported hardware

Code added in this phase:
- OpenGL 4.3 first-pass initialization with graceful fallback to 3.3
- Automatic backend selection between fragment and compute execution
- nvidia-compute backend activation and logging on NVIDIA hardware
- Grid-cell candidate lookup shared by both fragment and compute paths
- A batch-versus-single benchmark for multi-layer GPU dispatch efficiency
- GPU regression coverage against the CPU renderer at representative slice resolution

### Phase 3 benchmark snapshot

| Case | Previous ms | After phase 3 ms | Change |
| --- | ---: | ---: | ---: |
| 1M GPU orthographic compare | 509.16 | 532.37 | -4.6% |
| 8-layer GPU batch total | 4303.15 | 1015.20 | 4.24x faster |
| Orthographic archive throughput | 3182.30 | 2887.87 | +9.3% |
| Orthographic archive vs mesh archive | 6787.08 | 2887.87 | 2.35x faster |

## Final measured summary

| Scenario | Baseline ms | Final ms | Net result |
| --- | ---: | ---: | --- |
| 1M orthographic benchmark | 427.06 | 480.03 | Slight regression on tiny single slices |
| 10M orthographic benchmark | 5184.11 | 4544.66 | 1.14x faster |
| 8-layer GPU batch workload | 4303.15 | 1015.20 | 4.24x faster |
| End-to-end orthographic archive generation | n/a | 2887.87 | 2.35x faster than mesh archive path |

## Outcome (phases 1-3)

The full roadmap is now represented in the implementation through the portable OpenGL stack: cached GPU geometry, GPU spatial indexing, batched layer work, reduced readback bandwidth, overlapping CPU encoding, a compute-shader backend, and a vendor-tuned NVIDIA fast path. The strongest measured gains now show up on realistic multi-layer and end-to-end archive workloads, which is where the slicing pipeline spends most of its time.

---

## Phase 4: Corrected GPU benchmarks

Date: 2025-07-22

### Critical bugs found

Two bugs were discovered that invalidated all prior GPU benchmark data:

1. **Compute shader never activated**: In `GlSliceProjectionContext.CompileShaders()`, the `supportsComputeShaders` flag and `renderBackend` string were set to `false` / `"nvidia-fragment"` BEFORE the compute shader try-block but were NEVER updated on successful compilation. The compute shader code was compiled and linked successfully, but the runtime always fell back to the fragment shader path. Fixed by setting `supportsComputeShaders = true` and `renderBackend = "nvidia-compute"` after successful link.

2. **GPU path never invoked in benchmarks**: `OrthographicProjectionSliceBitmapGenerator.EnableGpuSliceProjection` is `false`, so `TryRenderGpuBatch` always returned null. All phase 1-3 "GPU" benchmarks actually measured CPU performance via the orthographic projection fallback path. The benchmark tests have been updated to call `gpuContext.TryRenderBatch` directly to bypass this flag.

### Additional fixes

- **GPU dedup epsilon**: Fragment and compute shaders used `kDedupEpsilon = 0.0005`. CPU interval-fill dedup uses `0.002`. Updated GPU epsilon to `0.001` (compromise - GPU per-pixel winding sum is more sensitive to epsilon than CPU interval fill; 0.002 caused correctness regression at IoU < 0.90 in dense scenes).
- **Triangle limit**: `MaxGpuTriangleCount` raised from 250,000 to 2,000,000 to allow benchmarking at 1M triangles.

### Corrected single-layer CPU vs GPU benchmarks

Direct `TryRenderBatch` calls, bypassing `EnableGpuSliceProjection` flag. NVIDIA GPU with compute shader backend confirmed active.

#### 100K active triangles (10x10x10 cuboid grid)

| Resolution | CPU Ortho ms | GPU Compute ms | GPU/CPU ratio | Result |
| --- | ---: | ---: | ---: | --- |
| 96x96 | 49 | 237 | 4.84x | GPU 4.8x slower |
| 480x300 | 72 | 91 | 1.27x | GPU 1.3x slower |
| 960x600 | 137 | 117 | 0.86x | **GPU 1.2x faster** |
| 1920x1200 | 375 | 194 | 0.52x | **GPU 1.9x faster** |
| 3840x2400 | 1332 | 563 | 0.42x | **GPU 2.4x faster** |

#### 1M active triangles (22x22x22 cuboid grid)

| Resolution | CPU Ortho ms | GPU Compute ms | GPU/CPU ratio | Result |
| --- | ---: | ---: | ---: | --- |
| 96x96 | 492 | 1093 | 2.22x | GPU 2.2x slower |
| 480x300 | 473 | 858 | 1.82x | GPU 1.8x slower |
| 960x600 | 584 | 1093 | 1.87x | GPU 1.9x slower |
| 1920x1200 | 859 | 2380 | 2.77x | GPU 2.8x slower |
| 3840x2400 | 1844 | 6065 | 3.29x | GPU 3.3x slower |

#### Key observations

- **GPU crossover point**: At 100K triangles, GPU becomes faster than CPU above ~800x500 resolution. At production resolution (3840x2400), GPU is 2.4x faster.
- **Triangle count scaling**: GPU performance degrades sharply with triangle count. At 1M triangles, the per-pixel winding sum evaluates more triangles per pixel through the spatial grid, and the GPU is 1.8x-3.3x slower at all resolutions.
- **CPU resolution scaling**: CPU time scales roughly linearly with pixel count (49ms at 96x96 to 1332ms at 3840x2400 for 100K triangles), consistent with per-row interval fill.
- **GPU resolution scaling**: GPU time at 100K triangles is nearly flat from 96x96 to 960x600 (overhead-dominated), then scales sub-linearly to 3840x2400 - the GPU excels when there is enough parallel work to amortize dispatch and readback overhead.

### Corrected batch benchmarks (16 layers, 1M triangles)

| Resolution | CPU batch ms | GPU batch ms | GPU/CPU ratio | Result |
| --- | ---: | ---: | ---: | --- |
| 96x96 | 2609 | 4281 | 1.64x | GPU 1.6x slower |
| 480x300 | 2767 | 4290 | 1.55x | GPU 1.6x slower |
| 960x600 | 3343 | 7662 | 2.29x | GPU 2.3x slower |
| 1920x1200 | 5078 | 24795 | 4.88x | GPU 4.9x slower |

GPU batch performance is poor at 1M triangles: sequential dispatch + readback per layer compounds the per-layer overhead, and batch amortization does not overcome the high per-pixel triangle evaluation cost.

### Corrected outcome

The previous phases (1-3) implemented the correct GPU infrastructure (spatial indexing, compute shaders, batching, NVIDIA tuning), but the benchmark data in those phases actually measured CPU performance due to the two bugs above.

With the bugs fixed and direct GPU benchmarks collected:

| Scenario | CPU ms | GPU ms | Net result |
| --- | ---: | ---: | --- |
| 100K tris, 3840x2400, single layer | 1332 | 563 | **GPU 2.4x faster** |
| 100K tris, 1920x1200, single layer | 375 | 194 | **GPU 1.9x faster** |
| 1M tris, 3840x2400, single layer | 1844 | 6065 | GPU 3.3x slower |
| 1M tris, 16 layers, 1920x1200 | 5078 | 24795 | GPU 4.9x slower |

### Recommendation

Keep `EnableGpuSliceProjection = false` (GPU disabled by default). The GPU path should be enabled adaptively based on runtime conditions:

- **Use GPU when**: active triangle count per layer is below ~200K AND output resolution is >= 960x600
- **Use CPU when**: active triangle count exceeds ~200K, resolution is low, or GPU is unavailable

The original `MaxGpuTriangleCount = 250,000` limit was well-calibrated to this crossover point. The limit has been raised to 2M for benchmarking flexibility, but the adaptive selector should cap GPU dispatch at ~200K active triangles.

### Future optimization opportunities

1. **Adaptive GPU/CPU selection**: Implement runtime triangle count check before each layer dispatch; route to GPU only when profitable
2. **GPU triangle culling**: The compute shader evaluates all triangles in a grid cell per pixel. A tighter per-workgroup bounding box cull or hierarchical grid could reduce wasted work at high triangle counts
3. **Multi-layer compute dispatch**: Render multiple Z heights in a single dispatch using layered framebuffers or 3D textures to amortize readback
4. **Async readback**: Use PBOs or persistent mapped buffers to overlap readback with the next layer's dispatch
5. **Mesh decimation**: For models exceeding 200K triangles, decimate before GPU dispatch (lossy but may be acceptable for preview slices)

## Files changed

- backend/Services/GlSliceProjectionContext.cs
- backend/Services/OrthographicProjectionSliceBitmapGenerator.cs
- backend/Services/PlateSliceRasterService.cs
- backend/Services/IPlateSliceBitmapGenerator.cs
- backend.Tests/PlateSliceRasterServiceTests.cs
- backend.Tests/SlicePerformanceScalingTests.cs
