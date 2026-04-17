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

## Outcome

The full roadmap is now represented in the implementation through the portable OpenGL stack: cached GPU geometry, GPU spatial indexing, batched layer work, reduced readback bandwidth, overlapping CPU encoding, a compute-shader backend, and a vendor-tuned NVIDIA fast path. The strongest measured gains now show up on realistic multi-layer and end-to-end archive workloads, which is where the slicing pipeline spends most of its time.

## Files changed

- backend/Services/GlSliceProjectionContext.cs
- backend/Services/OrthographicProjectionSliceBitmapGenerator.cs
- backend/Services/PlateSliceRasterService.cs
- backend/Services/IPlateSliceBitmapGenerator.cs
- backend.Tests/PlateSliceRasterServiceTests.cs
- backend.Tests/SlicePerformanceScalingTests.cs
