# Method 1 - Mesh-plane intersection and raster fill

## Overview
For each layer height $h$, intersect the triangle mesh with a horizontal plane, collect the resulting line segments, rebuild 2D contours, and fill those contours into a bitmap.

## Why it works well
This is close to how classic slicers reason about geometry. It preserves real mesh boundaries and maps naturally to PNG output.

## Pros
- High geometric accuracy for watertight meshes
- No GPU requirement
- Deterministic and testable
- Easy to map to printer pixel resolution

## Cons
- Needs careful handling of edge cases such as shared vertices and coplanar triangles
- Can become slower on very dense meshes with many layers
- Polygon joining and hole detection can get tricky

## Expected speed
- Small to medium plates: good
- Large detailed plates: moderate
- Complexity grows roughly with triangle count times layer count

## Implementation complexity
Medium.

## Concrete implementation plan
1. Load all plate meshes and apply placement transforms.
2. For each slice height, intersect every triangle against the plane.
3. Convert the resulting segments into scanline intersections or closed polygons.
4. Fill the inside region into an image buffer.
5. Save each layer as a PNG using the selected printer pixel resolution.
6. Package all layers and metadata into a ZIP archive.

## Current status
This is the method selected for the initial implementation in the backend.
