# Method 3 - Voxel occupancy grid slicing

## Overview
Convert the plate volume into a 3D voxel grid, mark occupied cells, and emit each Z layer as a 2D bitmap.

## Pros
- Robust against some non-manifold or messy input meshes
- Simple mental model once voxelization is complete
- Easy to apply morphological operations like dilation or erosion

## Cons
- High memory usage at fine resolution
- Can lose detail if the voxel size is too large
- Often slower overall than direct triangle slicing

## Expected speed
- Preprocessing cost can be high
- Per-layer output after voxelization is fast
- Memory cost rises sharply as pixel density increases

## Implementation complexity
High.

## Concrete implementation plan
1. Define a 3D voxel grid based on bed size and layer height.
2. Voxelize each placed mesh into the grid.
3. For each layer, write occupied cells into a monochrome image.
4. Optionally apply cleanup filters to reduce aliasing and holes.
5. Save PNG layers and archive them.

## Best use case
Repair-tolerant workflows where robustness matters more than exact mesh fidelity.
