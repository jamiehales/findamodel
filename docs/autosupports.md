# Auto-Support Generation

## Overview

The auto-support system analyzes unsupported 3D models and generates support-point markers
to identify where resin supports should be placed. It uses orthographic projection slicing,
island detection, and per-support pull force evaluation with configurable support tip sizes.

## Architecture

### Services

- **AutoSupportGenerationService** - core algorithm that slices geometry layer-by-layer,
  detects islands via flood-fill, evaluates pull forces, and places support points with
  appropriate sizing.
- **AutoSupportJobService** - manages asynchronous job lifecycle (queued, running,
  completed, failed) and caches results as binary envelopes.
- **AppConfigService** - persists and validates all auto-support tuning parameters.

### Data Flow

1. User triggers auto-support generation via `POST /api/models/{id}/auto-support/jobs`
2. `AutoSupportJobService` loads model geometry and calls `GenerateSupportPreview`
3. The algorithm slices the model into horizontal layers using orthographic projection
4. For each layer, islands (disconnected regions) are found via BFS flood-fill
5. Supports are placed and sized per-island based on pull force and density analysis
6. Results are returned as `SupportPoint[]` with geometry encoded in binary envelope format
7. Frontend renders support spheres with pull-force arrows colored by magnitude

## Algorithm

### Layer Slicing

The model is divided into horizontal layers. Voxel size and layer height are auto-calculated
from model dimensions but clamped to configured min/max bounds. Each layer is rendered to a
bitmap using orthographic projection.

### Island Detection

Connected lit pixels in each layer bitmap are grouped into islands using BFS. Each island
tracks its pixel list, boundary points, centroid, area, and bounding radius. Islands below
`MinIslandAreaMm2` are discarded.

### Support Placement

1. **Initial support** - each island without an existing support gets one at its centroid.
   Supports near the model base (bottom 15% or first 3mm) default to Heavy size; others
   start as Medium.

2. **Reinforcement loop** - iterates up to `MaxSupportsPerIsland` times per island:
   a. Evaluate pull forces for all supports assigned to the island
   b. Find uncovered pixels beyond `MaxSupportDistanceMm`
   c. Compute per-support capacity: `maxCapacity = pi * tipRadius^2 * resinStrength`
   d. If the strongest pull force exceeds capacity:
      - Count nearby supports within `MaxSupportDistanceMm`
      - If density is high (>= 3 neighbors): upgrade the support to the next larger size
      - If density is low: add a new support point
   e. If only coverage gaps remain (no force overload): add supports for coverage

3. **Support sizing** - four tip sizes are available (Micro, Light, Medium, Heavy), each
   with a configurable tip radius. New reinforcement supports start as Light (or Heavy
   near the base). Supports can be upgraded through the reinforcement loop.

### Pull Force Calculation

For each support, pixels in the island are assigned to their nearest support using Voronoi
partitioning. The pull force vector has three components:

- **Vertical**: `sqrt(supportedAreaMm2)` - proportional to the area the support covers
- **Lateral X/Z**: `(centroid - supportPos) * 0.35 * vertical` - torque from off-center load
- **Score**: `vectorLength + averageDistanceMm * 1.5` - combined force + distance penalty

The score is compared against per-support capacity (`pi * r^2 * resinStrength`) rather than
a single global threshold. This means larger tips can handle proportionally more force.

## Support Sizes

| Size   | Default Tip Radius | Relative Capacity |
|--------|-------------------|-------------------|
| Micro  | 0.4 mm            | ~0.50             |
| Light  | 0.7 mm            | ~1.54             |
| Medium | 1.0 mm            | ~3.14             |
| Heavy  | 1.5 mm            | ~7.07             |

Capacity = `pi * radius^2 * resinStrength` (default resinStrength = 1.0).

## Configuration

All parameters are stored in the `AppConfig` entity and exposed via the Settings page.

### Slicing Parameters
- **BedMarginMm** (0-20mm, default 2) - margin added around the model footprint
- **MinVoxelSizeMm** / **MaxVoxelSizeMm** - voxel size clamp range
- **MinLayerHeightMm** / **MaxLayerHeightMm** - layer height clamp range

### Support Placement Parameters
- **MergeDistanceMm** (0.1-25mm, default 2.5) - minimum distance between supports
- **MinIslandAreaMm2** (0-2500mm2, default 4) - islands below this area are skipped
- **MaxSupportDistanceMm** (merge distance-100mm, default 10) - max gap before adding supports
- **MaxSupportsPerIsland** (1-64, default 6) - iteration limit per island

### Support Sizing Parameters
- **ResinStrength** (0.1-10, default 1.0) - dimensionless resin strength multiplier
- **MicroTipRadiusMm** (0.1-3mm, default 0.4) - tip radius for Micro supports
- **LightTipRadiusMm** (0.1-5mm, default 0.7) - tip radius for Light supports
- **MediumTipRadiusMm** (0.1-7mm, default 1.0) - tip radius for Medium supports
- **HeavyTipRadiusMm** (0.1-10mm, default 1.5) - tip radius for Heavy supports

## Frontend Visualization

Support points are rendered as spheres in the 3D viewer with size proportional to their
tip radius. Pull force vectors are shown as arrows:

- Arrow direction follows the pull force vector
- Arrow color interpolates from yellow (low force) to red (high force)
- Arrow length scales with force magnitude
- Support size label is included in the point data for potential UI display

The supports visibility toggle in the nav bar controls both the sphere meshes and arrows.

## API

### Endpoints
- `POST /api/models/{id}/auto-support/jobs` - start a generation job
- `GET /api/models/{id}/auto-support/jobs/{jobId}` - poll job status and get support points
- `GET /api/models/{id}/auto-support/jobs/{jobId}/geometry` - download binary mesh envelope

### DTOs
- `AutoSupportPointDto` - position, radius, pull force vector, and size label
- `AutoSupportJobDto` - job status, progress, support count, and point list
