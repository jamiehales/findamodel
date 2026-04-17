# Method 2 - GPU orthographic layer rendering

## Overview
Render the model from above using an orthographic camera, clipping one thin slab per layer and writing the result directly into a texture the size of the printer LCD.

## Pros
- Very fast once the pipeline is set up
- Naturally outputs pixel-perfect masks
- Well suited for high-resolution printers and larger batches
- Can be extended to grayscale antialiasing

## Cons
- Requires a reliable graphics backend in headless environments
- Debugging GPU clipping and precision issues is harder
- More platform-specific behavior than a pure CPU slicer

## Expected speed
- Very high for repeated slicing
- Setup cost is higher, but per-layer throughput is excellent

## Implementation complexity
Medium to high.

## Concrete implementation plan
1. Upload merged plate geometry to a GPU buffer.
2. Create an orthographic render target at the printer resolution.
3. For each layer, apply a clipping range around the target height.
4. Render solid occupancy into a monochrome texture.
5. Read back or directly encode the texture into PNG.
6. Package the output into the same ZIP format as the CPU implementation.

## Best time to adopt
After the CPU version is stable and profiling shows layer generation is a bottleneck.
