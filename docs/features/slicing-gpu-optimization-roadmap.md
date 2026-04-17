# GPU slice optimization roadmap

## Status update
Implemented on 2026-04-16 and benchmarked in the companion report at [docs/features/slicing-gpu-optimization-benchmark-report.md](docs/features/slicing-gpu-optimization-benchmark-report.md).

Completed items in the codebase now include:
- GPU-resident cached geometry uploads across batched layer work
- Row-group GPU spatial indexing and tighter per-layer filtering
- Batched multi-layer dispatch through the orthographic GPU path
- Reduced readback bandwidth with single-channel slice outputs
- Overlapped PNG encoding and ZIP packaging
- Packed GPU upload formats and vendor-tuned row grouping
- Automated regression and throughput benchmarks

## Overview
This document lists the next optimizations for the GPU-backed slice path now that the first working implementation is in place.

The current implementation proves that the GPU path works and produces the same bitmap result as the CPU implementation for the benchmark harness. The next work should focus on reducing CPU to GPU transfer cost, increasing GPU occupancy, and avoiding unnecessary per-layer setup.

## Current bottlenecks
- Full triangle data is uploaded again for each rendered layer
- The fragment shader still loops over many triangles per pixel
- There is no spatial acceleration structure on the GPU
- The pipeline reads the full texture back to the CPU after every layer
- Only one layer is rendered at a time
- The current implementation uses a conservative OpenGL path rather than a more specialized compute path

## Future optimizations

### 1. Persist geometry on the GPU
Upload the mesh once per export job and keep it resident in GPU memory for all layers.

Expected benefit:
- Removes repeated buffer and texture upload cost
- Helps especially for large meshes and tall prints

Implementation notes:
- Cache GPU buffers by export job id or mesh hash
- Release them after archive generation completes
- Reuse the same texture or buffer handles across all layers

### 2. Add a GPU spatial index
Build a uniform grid, BVH, or layered triangle bins so each pixel only checks a small subset of triangles.

Expected benefit:
- Largest likely speedup for complex models
- Reduces the per-pixel triangle loop substantially

Implementation notes:
- Start with a simple fixed grid in bed space or Z bins per layer range
- Move to BVH if profiling shows the grid is not selective enough
- Precompute on the CPU first, then upload compact node data to the GPU

### 3. Move from fragment shader raster logic to compute shaders
Replace the current full-screen fragment approach with a compute shader designed specifically for slice occupancy evaluation.

Expected benefit:
- Better control over workgroup sizing and memory access
- Easier to batch multiple rows or tiles efficiently
- Better foundation for later CUDA or Vulkan work

Implementation notes:
- Dispatch by tile or row block
- Use shared local memory for triangle batches
- Tune workgroup sizes per vendor and resolution

### 4. Batch multiple layers per dispatch
Instead of rendering one layer at a time, process several adjacent layers in one job.

Expected benefit:
- Reduces repeated command submission overhead
- Improves reuse of triangle filtering and cached state

Implementation notes:
- Group layers in small batches such as 4, 8, or 16
- Reuse the same filtered triangle set where the Z overlap is similar
- Keep memory use bounded for tall print stacks

### 5. Avoid unnecessary full texture readback
Read back less data, or defer readback until a batch of layers is complete.

Expected benefit:
- Reduces PCIe transfer overhead
- Improves throughput on high-resolution printers

Implementation notes:
- Use pixel buffer objects for asynchronous readback
- Double-buffer the output surfaces so GPU work and CPU PNG encoding overlap
- Consider writing directly into a monochrome packed format before PNG encoding

### 6. Parallelize PNG encoding with GPU rendering
While the GPU renders the next layer or batch, encode and archive the previous results on CPU worker threads.

Expected benefit:
- Better end-to-end throughput
- Keeps both CPU and GPU busy at the same time

Implementation notes:
- Use a producer consumer queue between slice rendering and ZIP packaging
- Bound the queue size to avoid excessive memory growth

### 7. Pre-filter triangles by layer range on CPU before GPU submission
Use the existing layer bucketing system to send only the triangles relevant to a slice or slice batch.

Expected benefit:
- Reduces shader input size dramatically for tall models
- Works well with the current architecture and is low risk

Implementation notes:
- Keep per-layer or per-batch index lists
- Avoid duplicating full triangle buffers where possible
- Prefer indexed views into a shared vertex store

### 8. Use packed data formats
Reduce bandwidth by storing triangle data in tighter GPU-friendly formats.

Expected benefit:
- Faster upload and lower VRAM use
- Better cache behavior in the shader

Implementation notes:
- Evaluate half precision where safe
- Separate static geometry data from per-layer constants
- Pack bounds and vertex deltas instead of full absolute coordinates where appropriate

### 9. Add vendor-specific fast paths
Introduce optional advanced backends for NVIDIA hardware once the generic GPU path is stable.

Expected benefit:
- Potentially much larger speedups on RTX-class GPUs
- Better long-term path for very high resolution LCD printers

Implementation notes:
- Consider CUDA for raw compute kernels
- Consider OptiX if ray-style intersection remains the chosen approach
- Keep OpenGL or compute-shader fallback for portability

### 10. Add automated GPU profiling and regression benchmarks
Track GPU slicing time, upload cost, readback cost, and PNG encoding cost independently.

Expected benefit:
- Makes future optimization decisions evidence-based
- Prevents regressions from slipping into the pipeline

Implementation notes:
- Record timings for upload, dispatch, readback, encoding, and ZIP packaging separately
- Add a benchmark mode for representative small, medium, and large meshes
- Store baseline numbers for the 3080 Ti test machine

## Recommended implementation order
1. Persist geometry on the GPU
2. Pre-filter triangles by layer range
3. Parallelize encoding and readback overlap
4. Add a GPU spatial index
5. Move to compute shaders
6. Batch multiple layers per dispatch
7. Explore NVIDIA-specific backends if still needed

## Expected result after these steps
The short-term goal should be moving from a modest speedup to a clearly material one for real print jobs, especially at full printer resolution and across hundreds or thousands of layers.

The medium-term target should be to make the GPU path the default for supported hardware while keeping the CPU slicer as a reliable fallback.
