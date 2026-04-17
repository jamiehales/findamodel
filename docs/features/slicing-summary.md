# 3D printer slicing investigation summary

## Goal
Evaluate practical ways to generate printable slice images for resin style printers, with the near term implementation exporting a ZIP containing per-layer PNG files at the printer panel resolution.

## Recommended current path
Use mesh-plane intersection plus 2D raster fill.

This approach:
- works directly from the existing loaded triangle mesh
- maps cleanly to per-layer PNG output
- has moderate implementation complexity
- does not require GPU or external tools
- is easy to validate with unit tests and archive inspection

## Compared methods

| Method | Speed | Complexity | Accuracy | Best fit |
| --- | --- | --- | --- | --- |
| Mesh-plane intersection and raster fill | Medium | Medium | High | Best near term choice |
| GPU orthographic layer rendering | High | Medium to High | High | Best later for performance |
| Voxel occupancy slicing | Low to Medium | High | Medium | Good for repair-tolerant workflows |
| External slicer integration | High dev leverage | Medium | Very High | Best if full print prep is needed |

## Current implementation decision
The code now follows the first approach and exports:
- one ZIP archive
- a manifest file with resolution and layer settings
- a sequence of PNG layer images sized to the selected printer configuration

## Next recommended upgrades
1. Add configurable layer height per export.
2. Support antialiasing and grayscale exposure masks.
3. Add hollowing, drain holes, and support generation as separate stages.
4. Move heavy raster work to a background pipeline or GPU path if larger plates become slow.
