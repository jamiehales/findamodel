# Auto-Support Generation

## Overview

The auto-support system analyzes unsupported 3D models and generates support-point markers
to identify where resin supports should be placed. It uses orthographic projection slicing,
island detection, and per-support pull force evaluation with configurable support tip sizes.

## Architecture

### Services

- **AutoSupportGenerationV3Service** - current auto-support algorithm that slices geometry layer-by-layer,
  detects unsupported overhang pixels against the layer below, and places/reinforces support points.
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
Each setting directly controls a specific aspect of the auto-support generation algorithm.

### Slicing Parameters

These settings control how the model is sliced into horizontal layers for analysis.

- **Bed margin (mm)** (0-20mm, default 2) - extra space added around the model footprint when
  calculating the analysis bitmap. The model's X and Z dimensions are each increased by
  `BedMarginMm * 2` to give the projection some breathing room. Increasing this adds padding
  around the edges but reduces the effective resolution of the analysis grid for a given
  pixel count.

- **Min voxel size (mm)** / **Max voxel size (mm)** (default 0.8 / 2.0) - the algorithm
  auto-calculates the voxel (pixel) size by dividing the model's largest horizontal dimension
  by 48. This value is then clamped to the min/max range. Smaller voxel sizes increase
  analysis resolution and detect finer geometry, but increase processing time and memory.
  Larger values run faster but may miss small overhangs. Each pixel in the layer bitmap
  represents one voxel-sized square area.

- **Min layer height (mm)** / **Max layer height (mm)** (default 0.75 / 1.5) - the algorithm
  auto-calculates the layer height by dividing the model's vertical dimension by 48. This
  value is then clamped to the min/max range. Smaller layer heights analyze the model at
  more vertical positions, catching overhangs that start between layers. Larger values
  skip more vertical detail but run faster. Each layer is rendered at the midpoint of
  that layer slice.

### Support Placement Parameters

These settings control when and where new supports are added during the reinforcement loop.

- **Merge supports threshold (mm)** (0.1-25mm, default 2.5) - during the reinforcement loop,
  if the furthest unsupported pixel from any existing support exceeds this distance, a new
  support is added at that location. Decreasing this value increases support density by
  adding supports more aggressively to fill gaps. Increasing it allows larger unsupported
  spans before triggering a new support.

- **Min island area (mm2)** (0-2500mm2, default 4) - during island detection, connected pixel
  regions whose total area falls below this threshold are silently discarded. This filters
  out noise and tiny geometry features that do not need support. The area is calculated as
  `pixelCount * pixelAreaMm2` where pixel area comes from the voxel size. Setting this to 0
  means every detected island, no matter how small, will receive a support.

### Force and Capacity Parameters

These settings control the physics model that decides whether existing supports can handle
the load or need reinforcement.

- **Resin strength** (0.1+, default 1.0) - a dimensionless multiplier used in the support
  capacity formula: `capacity = pi * tipRadius^2 * resinStrength`. This scales how much
  load each support can carry. A value of 1.0 means the raw tip area determines capacity.
  Increasing this (e.g. for tougher resins) allows each support to carry more force before
  the algorithm adds reinforcement. Decreasing it (e.g. for brittle resins) makes the
  algorithm more conservative and generates more supports.

- **Crush force threshold** (0.1+, default 20.0) - the maximum compressive force allowed on
  any single support before the algorithm adds a reinforcement support. Compressive force is
  calculated from the angular moment of off-center pixels divided by the support tip radius.
  When any support's compressive force exceeds this value, a new support is placed at the
  furthest unsupported pixel. Lower values trigger reinforcement sooner, producing more
  supports.

- **Max angular force** (0.1+, default 40.0) - the maximum angular (tipping) force allowed on
  any single support. Angular force accumulates as `peelForce * distanceToPixel` for each
  pixel assigned to a support via Voronoi partitioning. When any support's angular force
  exceeds this threshold, a reinforcement support is added. This setting prevents situations
  where a support handles a large area that is far from its tip, which would cause it to
  tip over in practice.

- **Peel force multiplier** (0.01-5.0, default 0.15) - converts pixel area to force. Each
  pixel's contribution to the peel force is `pixelAreaMm2 * peelForceMultiplier`. This
  represents how much force the FEP/screen film separation exerts per unit area of cured
  resin. Higher values model stickier films or resins, resulting in more supports. Lower
  values model easier separation, resulting in fewer supports.

### Support Tip Sizing Parameters

These settings control the physical dimensions of support tips. The tip radius directly
determines the contact area where the support meets the model surface.

- **Light tip radius (mm)** (0.1-5mm, default 0.7) - radius of Light-size support tips.
  Light supports are the default size for newly placed supports and have a capacity of
  approximately `pi * 0.7^2 * resinStrength = 1.54`. Light supports are used for most
  initial placements and low-load reinforcement (overload ratio below 1.25).

- **Medium tip radius (mm)** (0.1-7mm, default 1.0) - radius of Medium-size support tips.
  Capacity of approximately `pi * 1.0^2 * resinStrength = 3.14`. Medium supports are
  assigned when the reinforcement loop detects moderate overload (overload ratio between
  1.25 and 1.8). They leave a slightly larger mark on the model surface but carry roughly
  twice the load of a Light support.

- **Heavy tip radius (mm)** (0.1-10mm, default 1.5) - radius of Heavy-size support tips.
  Capacity of approximately `pi * 1.5^2 * resinStrength = 7.07`. Heavy supports are
  assigned when the reinforcement loop detects severe overload (overload ratio above 1.8).
  They leave the largest mark but carry roughly 4.6 times the load of a Light support.

### Advanced Force Model Parameters

These settings control additional physical forces beyond basic peel force that improve
support placement accuracy for complex geometries.

#### Suction Force

Enclosed regions (cups, hollows) create vacuum during FEP separation, requiring extra
support. The algorithm detects enclosed pixels using BFS flood-fill from bitmap edges -
any lit pixel not reachable from the edge is considered enclosed.

- **Suction multiplier** (1-10, default 3.0) - force multiplier applied to supports covering
  enclosed (cupped) regions. When an island has enclosed pixels (detected via flood-fill),
  supports assigned to those pixels receive this multiplier on their pull force. Higher
  values place more supports inside cups and hollows. A value of 1 disables the suction
  boost.

#### Area Growth Force

Rapidly expanding cross-sections create higher peel forces because more resin cures per
layer. The algorithm tracks total lit area per layer and computes the growth ratio between
consecutive layers.

- **Area growth threshold** (0.1-5.0, default 0.5) - the minimum layer-to-layer area increase
  ratio that triggers additional force. A ratio of 0.5 means the layer area must grow by 50%
  compared to the previous layer. Lower values trigger more aggressively on smaller expansions.

- **Area growth multiplier** (1-5, default 1.5) - force multiplier applied to supports on
  layers where area growth exceeds the threshold. The multiplier scales with how much the
  growth ratio exceeds the threshold. This causes the algorithm to add more supports on
  flared or mushroom-shaped geometry.

#### Gravity Loading

Accumulated part weight above each support creates compressive force. The algorithm walks
layers bottom-up, accumulating resin mass per support based on assigned pixel area and
resin density.

- **Gravity loading enabled** (default: on) - enables the gravity accumulation pass after
  the main reinforcement loop. When enabled, supports that accumulate too much weight are
  upgraded to larger tip sizes. When disabled, only peel and lateral forces determine
  support sizing.

#### Hydrodynamic Drag

Thin, narrow features experience lateral forces during FEP separation as resin flows
around them. The algorithm identifies islands whose minimum bounding-box width is below
the configured threshold and applies a lateral drag force.

- **Drag coefficient multiplier** (0-5, default 0.5) - scales the lateral drag force applied
  to narrow features. Higher values generate more supports on thin walls and fins. A value
  of 0 disables drag force entirely.

- **Min feature width (mm)** (0.1-10mm, default 1.0) - features narrower than this threshold
  receive drag force. The width is the smaller dimension of the island's axis-aligned
  bounding box converted to mm. Increasing this value applies drag to wider features.

#### Thermal Shrinkage

UV-cured resin shrinks as it polymerizes, pulling inward from edges. Large flat areas are
most affected because shrinkage stress accumulates across the span. The algorithm identifies
large flat islands (area > 25mm2, low perimeter-to-area ratio) and places additional
supports biased toward edges.

- **Shrinkage percent** (0-15%, default 5.0) - volumetric shrinkage of the resin. Higher
  values cause the algorithm to add more edge supports on large flat areas. A value of 0
  disables shrinkage-based support placement entirely.

- **Shrinkage edge bias** (0-1, default 0.7) - controls how strongly additional shrinkage
  supports are biased toward island edges versus interior. A value of 1.0 places all
  shrinkage supports on the perimeter. A value of 0.0 distributes them evenly. The default
  of 0.7 places most supports near edges where shrinkage stress is highest.

### How the reinforcement loop uses these settings together

For each island of unsupported pixels:

1. All supports in range are identified and each pixel is assigned to its nearest support
   (Voronoi partitioning).
2. Per-support vertical pull force = `assignedPixelCount * pixelAreaMm2 * PeelForceMultiplier`.
3. Per-support angular force = sum of `peelForce * distanceFromPixelToSupport` for each pixel.
4. Per-support compressive force = `angularForce / tipRadius`.
5. Combined capacity = sum of `pi * tipRadius^2 * ResinStrength` across all supports on
   the island.
6. The algorithm checks four conditions to decide if reinforcement is needed:
   - Total vertical pull exceeds combined capacity
   - Any support's compressive force exceeds `CrushForceThreshold`
   - Any support's angular force exceeds `MaxAngularForce`
   - The furthest pixel distance exceeds `MergeSupportsThreshold` (spacing)
7. If reinforcement is needed, a new support is placed at the furthest unsupported pixel.
   Its size (Light/Medium/Heavy) is chosen based on the overload ratio - the maximum of
   the load ratio, crush ratio, and angular ratio.

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
