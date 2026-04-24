# Support Algorithm Research - Missing Forces and Functionality

Research into forces and phenomena relevant to resin (MSLA/DLP/SLA) 3D printing support
generation that are not currently modeled in the auto-support algorithm.

## Current algorithm summary

The current algorithm models:
- **Peel force** (vertical) - proportional to cross-section area per layer via a flat multiplier
- **Angular/tipping force** - moment arm from off-center pixels
- **Compressive force** - angular force divided by tip radius
- **Support capacity** - `pi * tipRadius^2 * resinStrength`
- **Spacing coverage** - ensuring no pixel is too far from a support

---

## 1. Suction cup / vacuum effect during FEP separation

### What it is
When a cured layer separates from the FEP film, enclosed or concave geometry creates a
suction cup effect. A hollow cup shape, for example, resists separation far more than
a flat cross-section of equal area because air/resin cannot flow in to equalize pressure
during peel. The peel force for enclosed regions can be 3-10x higher than for open geometry
of the same cross-section area.

### Why it matters
The current algorithm treats all pixels equally via `pixelAreaMm2 * peelForceMultiplier`.
A 10mm diameter solid circle and a 10mm diameter hollow cup have roughly similar pixel
counts but wildly different actual peel forces. This is one of the most common causes of
mid-print support failures.

### How to detect it
- For each layer, identify closed contours (pixels whose boundary forms a complete loop)
- Compute the enclosed volume between the current layer and the FEP
- Regions where the cured perimeter fully surrounds uncured interior (or where the shape
  has concave pockets facing the build plate) are suction-prone

### Implementation approach
- During island detection, classify island boundary topology: open vs closed contour
- For closed contours, compute an "enclosure ratio" = interior area / perimeter length
- Apply a suction multiplier to peel force for enclosed islands (e.g. 2-5x based on
  enclosure depth and area)
- Alternatively, run a flood fill from the bitmap edges to find pixels reachable by
  resin flow; unreachable interior regions get the suction penalty

### Complexity
Medium. Requires contour analysis on the existing bitmap, which is already available.
The flood-fill approach from bitmap edges is straightforward and reuses existing BFS
infrastructure.

### Pros
- Addresses one of the most common real-world failure modes
- Minimal additional data structures (operates on existing layer bitmaps)
- Can be approximated cheaply without full fluid simulation

### Cons
- Suction force magnitude is resin/FEP dependent and hard to calibrate precisely
- The multiplier would need a new setting to tune
- Real suction depends on peel speed and FEP flexibility, which are printer-dependent

### Sources
- Formlabs white paper on peel mechanics in bottom-up SLA (formlabs.com/blog)
- Pan et al., "Separation force analysis in constrained-surface stereolithography",
  Rapid Prototyping Journal, 2017
- Community consensus on r/resinprinting regarding hollow model failures

---

## 2. Layer cross-section area change rate (delta area force)

### What it is
Abrupt increases in cross-section area between consecutive layers create higher peel forces
than gradual changes. A layer that is much larger than the previous layer has more newly-cured
area bonded to the FEP, while the model above it has less structural mass to resist the peel.
This is distinct from a simple large cross-section - a large cross-section that has been
consistent for many layers has accumulated structural support, while a sudden expansion does not.

### Why it matters
The current algorithm evaluates each layer independently and only checks whether pixels are
unsupported (absent from the layer below). It does not account for how much new area appeared
compared to the previous layer. A layer that doubles in area represents a high-risk peel
event even if all pixels technically overlap with the layer below.

### How to detect it
- Compare pixel count (or area) between layer N and layer N-1
- Compute delta: `(areaN - areaN-1) / areaN-1`
- Flag layers where delta exceeds a configurable threshold (e.g. >50% increase)

### Implementation approach
- After slicing all layers, compute per-layer cross-section area
- For each island, track area growth rate across adjacent layers
- Apply an "area growth multiplier" to peel force for supports on rapidly-expanding layers
- Consider upgrading supports to heavier sizes at expansion transitions

### Complexity
Low. The data is already available (pixel counts per island per layer). Only requires
a comparison between adjacent layers.

### Pros
- Trivial computation on existing data
- Catches a real failure mode (gradual overhangs that suddenly widen)
- Easy to tune with a single threshold parameter

### Cons
- The relationship between area growth rate and actual peel force increase is not linear
- May over-support gradual curves that happen to cross the threshold boundary

### Sources
- Lychee slicer documentation on "critical angle" detection for support placement
- Chitubox "auto-support" algorithm description referencing cross-section change analysis
- Practical experience documented in 3D printing community forums

---

## 3. Gravity and model weight (cumulative mass loading)

### What it is
As a print progresses upward, the weight of cured resin above each support accumulates.
A support near the base of a tall model bears the gravitational load of all geometry
above it, not just the local layer's peel force. For tall or dense models, this cumulative
weight can exceed the structural strength of thin supports.

### Why it matters
The current algorithm evaluates forces per-layer and per-island independently. It does not
track the cumulative weight that each support must bear from all layers above it. A support
placed at layer 5 that holds up 100 layers of dense geometry is under far more gravitational
stress than one at layer 95 holding up 5 layers.

### How to detect it
- For each support, sum the volume (pixel count * voxel volume * resin density) of all
  layers above the support that are connected to it
- Compare cumulative weight against support capacity

### Implementation approach
- After initial support placement, do a top-down pass accumulating connected volume
  per support
- For each support, compute gravitational load: `connectedVolumeMm3 * resinDensityGPerMl * g`
- Add gravitational load to the peel force evaluation
- `AutoSupportResinDensityGPerMl` already exists as an unused legacy setting (default 1.25)
  and could be activated

### Complexity
Medium. Requires tracking connected geometry across layers (3D connectivity), which the
current algorithm does partially but doesn't propagate cumulative loads downward.

### Pros
- Prevents failures on tall prints where supports fail under accumulated weight
- The legacy `ResinDensityGPerMl` setting is already defined
- Models a real physical force that is always present

### Cons
- Requires 3D connectivity tracking across layers (not just 2D islands)
- Adds a second pass through all layers after initial placement
- Weight-based failures are less common than peel-based failures for typical miniature-scale
  prints, but become significant for larger models

### Sources
- Standard physics: gravitational load = mass * g
- Formlabs Form 3 documentation on "graduated support sizing" based on model height
- Engineering analysis of cantilever beam loading applied to support structures

---

## 4. Lateral forces from resin flow during peel (hydrodynamic drag)

### What it is
During the peel/separation phase, liquid resin flows laterally to fill the gap between
the FEP and the newly-cured layer. This flow exerts lateral drag forces on the model
and its supports, particularly on thin features or areas with restricted flow paths.

### Why it matters
The current algorithm models lateral force only as torque from off-center pixel loading.
It does not model the physical lateral drag from resin flowing during peel. Thin vertical
features (e.g. sword blades, antennae) can be bent or broken by hydrodynamic drag even when
vertical peel forces are adequately supported.

### How to detect it
- Identify thin features: islands with high aspect ratio (long and narrow)
- Identify restricted flow regions: narrow gaps between model features where resin
  must flow through during peel
- Compute exposed surface area perpendicular to the likely flow direction

### Implementation approach
- During island detection, compute aspect ratio and minimum width
- For narrow features, add a lateral force component proportional to:
  `featureHeightMm * featureWidthMm * dragCoefficient`
- For restricted flow regions, identify chokepoints in the bitmap where flow is constrained
- Apply additional lateral force to supports near chokepoints

### Complexity
Medium-High. Requires geometric analysis beyond simple pixel counting. The aspect ratio
calculation is simple, but flow restriction analysis requires more sophisticated bitmap
processing.

### Pros
- Addresses a real failure mode for detailed miniatures (thin swords, staffs, etc.)
- Aspect ratio detection is cheap and covers the most common case
- Could be approximated without full fluid dynamics

### Cons
- True hydrodynamic simulation is computationally expensive
- Drag coefficient is highly dependent on peel speed, resin viscosity, and layer height
- May require printer-specific calibration

### Sources
- Wu et al., "Analysis of separation force in constrained-surface stereolithography
  using finite element method", Journal of Manufacturing Processes, 2019
- Liravi et al., "Separation force analysis and prediction based on cohesive element
  modeling for constrained-surface stereolithography", Computer-Aided Design, 2015
- Community reports of thin feature breakage during printing

---

## 5. Thermal shrinkage and polymerization stress

### What it is
UV-cured photopolymer resin undergoes volumetric shrinkage during polymerization (typically
3-8% for standard resins). This creates internal stresses that pull geometry inward and can
warp thin sections, break supports, or delaminate layers. The shrinkage is not uniform - it
depends on local geometry, cure depth, and resin formulation.

### Why it matters
The current algorithm does not model shrinkage forces at all. For large flat cross-sections,
polymerization shrinkage creates significant internal stress that can curl edges upward
(like a bimetallic strip). This curling effect requires supports at the edges of large flat
areas, even when the geometry is technically connected to the layer below.

### How to detect it
- Large flat cross-sections (high pixel count, low perimeter-to-area ratio)
- Sudden temperature changes from UV exposure on large areas
- Thin sections that are more susceptible to warping from shrinkage

### Implementation approach
- Compute a "shrinkage risk" metric per island based on:
  - Island area (larger = more shrinkage stress)
  - Perimeter-to-area ratio (lower = more risk, as there is less edge constraint)
  - Distance from support points to island edges
- For large islands with low perimeter-to-area ratio, bias support placement toward edges
  rather than centroids
- Add a setting for shrinkage percentage (resin-dependent)

### Complexity
Medium. The metrics are calculable from existing island data. The main challenge is
determining the shrinkage force magnitude and how it translates to support requirements.

### Pros
- Addresses curling/warping, a very common failure mode for flat geometry
- Perimeter-to-area ratio is trivially computable from existing island data
- Edge-biased support placement is a small modification to the centroid-based placement

### Cons
- Shrinkage percentage varies significantly between resins (3-8%+)
- The relationship between shrinkage stress and actual deformation depends on layer thickness,
  cure energy, and surrounding geometry
- Difficult to validate without physical testing

### Sources
- Material data sheets from resin manufacturers (Elegoo, Anycubic, Siraya Tech) reporting
  volumetric shrinkage percentages
- Huang et al., "Curl distortion analysis during photopolymerisation of
  stereolithography using dynamic finite element method", International Journal of
  Advanced Manufacturing Technology, 2003
- Practical observation: large flat overhangs curl upward at edges during printing

---

## 6. Overhang angle sensitivity

### What it is
The angle at which geometry overhangs affects support requirements non-linearly. A 45-degree
overhang needs far less support than a 10-degree (nearly horizontal) overhang. Surface normal
analysis can determine the overhang angle of each region and adjust support density accordingly.

### Why it matters
The current algorithm only checks whether pixels are "unsupported" (absent from the layer
below). It treats all unsupported pixels equally regardless of the angle of the surface
they belong to. A gradually sloping 40-degree overhang and a nearly flat 5-degree shelf
get the same treatment, when in practice the 5-degree shelf needs much denser support.

### How to detect it
- Compute the surface normal at each triangle in the original mesh
- Project normal angles onto the vertical axis to get overhang angle
- Alternatively, approximate from layer bitmaps: compare the boundary expansion rate
  between consecutive layers

### Implementation approach
- **Mesh-based**: For each support point, sample the mesh surface normal at the support
  location. Compute overhang angle = acos(normal dot vertical). Apply a force multiplier
  that increases as angle approaches horizontal (e.g. `1 / sin(angle)`)
- **Bitmap-based**: For each island, compute how many new pixels appeared compared to the
  layer below. A high ratio of new-to-existing pixels indicates a shallow overhang angle.
  Apply a peel force multiplier proportional to this ratio.

### Complexity
Low (bitmap-based) to Medium (mesh-based). The bitmap approach is a simple extension of
existing per-layer analysis. The mesh-based approach requires triangle normal computation
but produces more accurate results.

### Pros
- Directly addresses the most fundamental physical relationship in support placement
- Bitmap-based approximation is trivially cheap
- Better overhang handling reduces both over-supporting (steep angles) and under-supporting
  (shallow angles)

### Cons
- Bitmap-based approach is an approximation; mesh normals are more accurate
- The force-angle relationship is not purely geometric (it also depends on layer height
  and resin properties)
- Requires careful calibration to avoid over-supporting near-vertical surfaces

### Sources
- Standard SLA support placement literature; all major slicers (Chitubox, Lychee, PrusaSlicer)
  use overhang angle as a primary support trigger
- Huang et al., "Slurry-based additive manufacturing of support structures using overhang
  angle", Additive Manufacturing, 2020
- ChiTuBox documentation: "Critical Angle" setting for auto-support

---

## 7. FEP tilt/peel kinematics (non-uniform separation)

### What it is
Most MSLA printers use a tilt-peel mechanism where the build plate lifts at a slight angle
before fully separating. This means separation starts at one edge and progresses across the
layer like peeling tape. The forces are not uniform across the layer - the edge where
separation begins experiences momentary peak forces, and geometry near the peel front
experiences higher forces than geometry already separated.

### Why it matters
The current algorithm models peel force as uniform across the entire layer. In reality,
the peel front creates a localized force concentration that sweeps across the layer. Geometry
at the leading edge of the peel needs more support than geometry at the trailing edge.

### How to detect it
- Requires knowledge of the printer's peel direction/axis (typically Y-axis)
- Geometry at the peel-start edge experiences higher forces

### Implementation approach
- Add a printer-specific "peel direction" setting (axis or angle)
- Apply a position-dependent peel force multiplier: higher at the peel-start edge,
  lower at the peel-end edge
- The multiplier could follow a gradient: e.g. 1.5x at peel start, 0.8x at peel end,
  with linear interpolation between them
- Could be combined with per-printer profiles

### Complexity
Low. Applying a positional gradient to an existing force calculation is simple arithmetic.
The main challenge is determining the correct gradient parameters.

### Pros
- Models a real physical phenomenon that affects all tilt-peel printers
- Very cheap computation (just a positional multiplier)
- Can reduce over-support on the trailing edge while increasing support at the leading edge

### Cons
- Peel direction varies between printers and some printers use linear lift (no tilt)
- The force gradient depends on peel speed, FEP tension, and layer adhesion
- Adds printer-specific configuration complexity
- Some printers have variable peel mechanisms (slow lift + fast lift)

### Sources
- Prusa SL1/S documentation on tilt mechanism and peel forces
- Creality Halot printer documentation on build plate lift mechanisms
- Community discussion of "peel direction" and its effect on print quality

---

## 8. Support-to-support structural interaction

### What it is
Multiple supports near each other interact structurally. Closely-spaced supports can share
load through the raft or base layer, forming a more rigid structure. Conversely, a cluster
of supports all bearing heavy load can cause localized raft detachment if the total force
exceeds the raft's adhesion to the build plate.

### Why it matters
The current algorithm evaluates each support's capacity independently (`pi * r^2 * resinStrength`).
It does not model how support clusters interact. Two Light supports 1mm apart can collectively
resist more lateral force than two Light supports 10mm apart because the close pair forms a
rigid truss. Conversely, heavy load concentration on a small raft area can cause plate
adhesion failure.

### How to detect it
- Compute pairwise distances between supports
- Identify support clusters (groups within a configurable proximity threshold)
- Calculate total load per unit raft area for each cluster

### Implementation approach
- After initial support placement, group supports by proximity
- For clusters, compute a "truss bonus" to lateral/angular force capacity based on the
  minimum spanning distance of the cluster
- For load concentration analysis, sum all vertical forces within a cluster and compare
  against raft adhesion capacity (raft area * adhesion strength)
- If raft overload is detected, spread supports outward or add more supports to distribute load

### Complexity
Medium. Clustering is O(n^2) for pairwise distances but n is small (typically <100 supports).
The truss modeling could be approximated simply.

### Pros
- Models real structural behavior of support clusters
- Catches raft adhesion failures, which are a common print failure mode
- Relatively small support counts keep computation cheap

### Cons
- Accurate truss analysis is complex (finite element analysis)
- Raft adhesion strength is printer/resin dependent and hard to calibrate
- Approximations may not capture the full structural behavior

### Sources
- Structural engineering principles for truss analysis
- Community reports of "raft peel" failures on heavy models
- Formlabs documentation on support density and raft design

---

## 9. Bridge and cantilever detection

### What it is
Bridges (horizontal spans between two supported regions) and cantilevers (horizontal
projections from a supported region) have fundamentally different support requirements than
overhangs. A bridge can be partially self-supporting if both ends are anchored, while a
cantilever concentrates all force at its root.

### Why it matters
The current algorithm treats all unsupported pixels equally. A bridge pixel midway between
two supports actually experiences less force than an equivalent cantilever pixel at the same
distance, because the bridge is anchored on both sides. Conversely, the root of a cantilever
experiences concentrated moment forces that can exceed the capacity calculation based on
simple area assignment.

### How to detect it
- For each unsupported island, analyze the connectivity to supported regions:
  - If connected on two or more sides: bridge (lower risk)
  - If connected on one side only: cantilever (higher risk)
  - If connected on no sides: floating island (highest risk, already handled)
- Compute cantilever length (maximum distance from anchored edge to free edge)

### Implementation approach
- During island analysis, identify anchor edges (pixels adjacent to supported layer-below pixels)
- Classify island topology: bridge vs cantilever vs floating
- For cantilevers, apply a moment arm multiplier to force: `forceMult = 1 + cantileverLengthMm / referenceLength`
- For bridges, reduce the required support density proportional to the bridge span ratio:
  `supportReduction = 1 - (anchoredPerimeterRatio * 0.3)`

### Complexity
Medium. Requires connectivity analysis between unsupported pixels and the layer below's
supported pixels. The topology classification is a graph analysis problem.

### Pros
- Reduces over-supporting of bridges (common in miniature bases, architectural models)
- Increases support for cantilevers (common in outstretched arms, wings, etc.)
- Uses existing pixel data, no new data structures needed

### Cons
- Classification heuristics may misidentify complex geometry
- The force reduction for bridges depends on bridge span and material stiffness
- Adds complexity to the per-island evaluation logic

### Sources
- Chitubox "bridge detection" feature description
- Lychee slicer documentation on cantilever support mode
- Standard structural engineering: beam bending moment analysis

---

## 10. Resin viscosity and drainage (trapped volume)

### What it is
When the build plate lifts between layers, liquid resin must flow in to fill the gap. Viscous
resins flow slowly, and trapped volumes (pockets or channels) may not fully drain before the
next exposure. Incomplete drainage can cause pressure differentials, trapped bubbles, and
localized over-cure.

### Why it matters
The current algorithm does not consider resin flow or drainage. Complex geometry with internal
channels or deep pockets may require additional supports to maintain structural rigidity
during the drainage phase, or may need vent holes that the algorithm could suggest.

### How to detect it
- Identify concave regions facing downward (inverted cups)
- Compute the maximum depth of trapped volumes
- Detect narrow channels where resin flow is restricted

### Implementation approach
- During layer analysis, identify downward-facing concavities by comparing consecutive layers
- For deep concavities (spanning multiple layers), flag as drainage concerns
- Suggest orientation changes or vent holes for severe cases
- Add additional support near drainage-restricted areas to counteract suction

### Complexity
Medium-High. Trapped volume detection requires multi-layer analysis and 3D reasoning about
resin flow paths. A simplified 2D approach per layer is feasible but less accurate.

### Pros
- Addresses a real failure mode, especially for hollow or complex prints
- Can be combined with suction cup detection (item 1) for compound benefit
- Useful as both a support placement input and a user warning

### Cons
- Full drainage simulation is computationally expensive
- Simplified heuristics may produce false positives
- Vent hole suggestion goes beyond support placement into model modification territory

### Sources
- Elegoo resin printer user manual recommendations on drainage holes
- Community guides on hollowing and drainage for resin prints
- Chitubox "hollow" feature documentation on automatic vent hole placement

---

## 11. Layer adhesion strength (inter-layer bonding)

### What it is
The bond strength between consecutive cured layers depends on exposure time, resin chemistry,
and layer height. Under-exposed layers have weak inter-layer bonds and can delaminate under
peel forces. The support algorithm could account for expected inter-layer bond strength to
determine how much load can be transferred between layers vs. how much must be borne by supports.

### Why it matters
The current algorithm assumes all cured geometry below a support provides adequate anchoring.
In practice, if the geometry between a support and the build plate has weak inter-layer bonds
(e.g. due to thin cross-sections or minimal overlap between layers), the support can delaminate
from the model even when its tip capacity is not exceeded.

### How to detect it
- Track the minimum cross-section area along the path from each support to the raft
- Identify "bottleneck" layers where the cross-section narrows significantly
- Compute inter-layer overlap ratio between consecutive layers

### Implementation approach
- After support placement, trace each support's load path downward through layers
- Compute the minimum cross-section area along the path
- If the bottleneck area is too small relative to the support's expected load, flag for
  additional support or warn the user
- Could also model inter-layer bond strength as `overlapAreaMm2 * bondStrengthPerMm2`

### Complexity
Medium-High. Requires tracing load paths through 3D layer connectivity, which is more
complex than the current per-layer independent analysis.

### Pros
- Catches delamination failures that are invisible to per-layer analysis
- The bottleneck detection is conceptually simple even if implementation is involved
- Could prevent catastrophic failures where an entire section separates mid-print

### Cons
- Bond strength depends on exposure settings, resin type, and ambient temperature
- Load path tracing through arbitrary geometry is complex
- May produce false warnings for geometry that is structurally sound due to lateral
  connectivity the algorithm cannot easily model

### Sources
- Resin manufacturer data sheets on inter-layer bond strength vs exposure time
- Photocentric documentation on layer adhesion testing methodology
- Community testing of inter-layer bond failure modes

---

## 12. Orientation-aware support accessibility

### What it is
Supports must be removable after printing. Supports placed in concavities, undercuts, or
between closely-spaced features may be impossible to reach with tools. The algorithm should
consider whether a support can physically be removed by the user.

### Why it matters
The current algorithm places supports purely based on mechanical need without considering
post-print accessibility. Supports in inaccessible locations can damage the model during
removal or may be left in place (degrading print quality).

### How to detect it
- For each candidate support location, check for line-of-sight to the model exterior
- Detect support points enclosed within geometry (surrounded by model walls)
- Check clearance between support tip and adjacent geometry

### Implementation approach
- After support placement, perform a ray-casting check from each support point outward
  to the model exterior
- Flag supports where no clear path exists for tool access
- For flagged supports, attempt to relocate to a nearby accessible position
- If no accessible position exists, warn the user

### Complexity
High. Ray-casting against the full mesh for each support point is computationally expensive.
Could be approximated by checking adjacent pixels in the bitmap for enclosed regions.

### Pros
- Improves print usability and post-processing quality
- Prevents model damage during support removal
- Reduces waste from unusable supports

### Cons
- Accurate accessibility analysis requires 3D ray-casting (expensive)
- "Accessibility" depends on the user's tools and skill level
- Bitmap approximation may miss 3D accessibility issues
- Moving supports for accessibility may compromise structural adequacy

### Sources
- Lychee slicer "support accessibility" check feature
- PrusaSlicer support blocker/enforcer tools for manual accessibility control
- Chitubox documentation on support removal best practices

---

## 13. Surface quality-aware tip placement

### What it is
Support tips leave marks on the model surface. The visibility and impact of these marks
depends on whether the surface is cosmetic (visible in the final model) or non-cosmetic
(hidden, internal, or on the base). Support placement could prefer non-cosmetic surfaces.

### Why it matters
The current algorithm places supports at centroids and high-force locations without
considering surface visibility. A support tip on a character's face leaves a more impactful
mark than one on the underside of a base, even though both locations may be equally valid
from a structural perspective.

### How to detect it
- Identify "upward-facing" surfaces (surface normal pointing away from build plate) as
  likely cosmetic
- Detect flat top surfaces and smooth curves
- User-provided markup (support blocker/enforcer zones) could override automatic detection

### Implementation approach
- Compute surface normal at each support candidate location
- Assign a "cosmetic penalty" to surfaces with normals facing the camera/viewer direction
  or with smooth curvature
- When multiple valid support positions exist, prefer the one with the lowest cosmetic penalty
- Support blocker zones (user-specified) should override all automatic placement

### Complexity
Medium. Surface normal analysis is available from the mesh. The cosmetic penalty heuristic
is subjective and hard to define precisely without user input.

### Pros
- Significantly improves print quality where it matters most (visible surfaces)
- Cosmetic placement is a key differentiator for premium slicers
- Can be partially automated with reasonable heuristics

### Cons
- "Cosmetic" is subjective; automatic detection will not be perfect
- May conflict with optimal structural placement
- Requires user-facing controls for override/customization

### Sources
- Lychee slicer "smart orientation" and surface quality analysis
- Formlabs PreForm "touchpoint optimization" feature
- Community discussion of support mark minimization techniques

---

## 14. Model orientation impact on support requirements

### What it is
The orientation of the model on the build plate dramatically affects the number and
placement of supports needed. A 45-degree rotation can halve the required supports by
eliminating large flat overhangs. Automatic orientation analysis could suggest optimal
orientations or warn about poor orientations.

### Why it matters
The current algorithm accepts the model orientation as-given and generates supports
accordingly. It does not suggest alternative orientations that might require fewer supports,
produce better surface quality, or reduce print time.

### How to detect it
- Sample multiple candidate orientations (e.g. 6 cardinal + 8 diagonal = 14 orientations)
- For each orientation, estimate support requirements: total unsupported area, number of
  islands, maximum overhang span
- Rank orientations by support count / quality tradeoff

### Implementation approach
- Implement a "quick estimate" mode that evaluates support need without full placement
- For each candidate orientation, compute: total unsupported pixel area, number of floating
  islands, maximum cross-section area (determines peel force), model height (determines
  print time)
- Present ranked orientations to the user with estimated support count and print time
- Optionally auto-select the optimal orientation

### Complexity
Medium-High. Requires multiple passes of the slicing pipeline (one per candidate orientation).
Could be made fast by using a very coarse voxel size for estimation.

### Pros
- Can dramatically reduce support count and improve print quality
- Coarse estimation can be fast enough for real-time feedback
- Major quality-of-life improvement for users

### Cons
- "Optimal" depends on user priorities (surface quality, print time, support count, resin usage)
- Multiple slicer passes increases computation time
- Orientation optimization is an N-dimensional problem with local minima

### Sources
- Chitubox Pro "auto-orientation" feature
- Lychee slicer "smart orientation" feature documentation
- Frank and Fadel, "Expert system-based selection of the preferred direction of build for
  rapid prototyping processes", Journal of Intelligent Manufacturing, 1995

---

## 15. Support structure type variation (pillar, tree, lattice)

### What it is
The current algorithm places point supports (single vertical pillars). Alternative support
structures include tree supports (branching structures sharing a single base), lattice
supports (interconnected grid), and cone supports (wide base tapering to a point). Different
geometries benefit from different support types.

### Why it matters
Point/pillar supports are simple but not always optimal. Tree supports can reach multiple
overhang points from a single base, reducing raft contact and resin usage. Lattice supports
provide continuous coverage under large flat areas. The current algorithm's single support
type limits optimization opportunities.

### How to detect it
- Large flat overhangs benefit from lattice supports
- Multiple nearby small islands at different heights benefit from tree supports
- Isolated overhangs are well-served by pillar supports

### Implementation approach
- After initial point-based placement, analyze support point clusters
- For clusters of supports near each other but at different heights, merge into tree supports
  (single trunk with branches to each support point)
- For large flat overhangs with many supports, convert to lattice pattern
- Tree generation: compute minimum spanning tree of support point positions, then generate
  geometry that follows the tree structure with configurable trunk/branch diameters

### Complexity
High. Tree and lattice support generation require significant new geometry generation code.
The placement algorithm is a separate concern from the geometry generation, but they interact.

### Pros
- Tree supports use less resin and leave fewer marks
- Lattice supports are more rigid and better for large flat areas
- Different support types for different needs is a significant feature improvement

### Cons
- Substantial new code for geometry generation
- Tree/lattice removal is more complex than pillar removal
- Interaction between support types adds algorithmic complexity
- May require changes to the binary envelope format

### Sources
- Chitubox tree support algorithm documentation
- Lychee slicer lattice and tree support modes
- Meshmixer tree support generation algorithm (open source reference)
- PrusaSlicer organic support generation (FDM but applicable concepts)

---

## 16. Raft design and build plate adhesion

### What it is
The raft (base layer connecting supports to the build plate) must provide adequate adhesion
without being impossible to remove. Raft design - thickness, area, edge geometry - affects
both adhesion strength and removability. The support algorithm should consider raft adequacy.

### Why it matters
The current algorithm places support points but does not explicitly model the raft's ability
to hold those supports to the build plate. A model with many heavy supports concentrated in
a small area may overwhelm the raft's adhesion capacity, causing plate detachment.

### How to detect it
- Compute the convex hull of all support base positions
- Compare total vertical force against raft adhesion estimate (area * adhesion strength)
- Check for force concentration (supports clustered in a small area)

### Implementation approach
- After support placement, compute raft footprint (convex hull of support base positions)
- Estimate raft adhesion capacity = raft area * adhesion strength per mm2
- If total expected peel force exceeds raft capacity, suggest:
  - Expanding raft area
  - Adding sacrificial base supports at raft edges for adhesion
  - Distributing supports more evenly

### Complexity
Low-Medium. Convex hull and area calculation are simple. The main challenge is calibrating
adhesion strength values.

### Pros
- Prevents complete print failures from plate detachment
- Simple computation on existing data
- Could provide useful warnings to users before starting a print

### Cons
- Build plate adhesion depends on plate surface condition, cleaning, and resin type
- Raft geometry generation is outside the current support placement scope
- Adhesion strength is highly variable and hard to predict

### Sources
- Prusa SL1 documentation on raft design and plate adhesion
- Elegoo printer user manuals on first-layer exposure and raft settings
- Community testing of build plate adhesion vs raft area and thickness

---

## 17. Dynamic support density based on print height

### What it is
The risk profile of a print changes with height. Early layers face the highest risk because:
the model has minimal structural mass, supports are at their longest (most flexible), and
any failure at the base ruins the entire print. Later layers are better anchored and have
shorter, stiffer supports.

### Why it matters
The current algorithm applies the same support criteria at all heights. It could be more
aggressive (more supports, larger tips) at the base and more relaxed (fewer supports,
smaller tips) higher up, reflecting the actual risk profile.

### How to detect it
- Support height = distance from support tip to raft
- Model height progress = current layer / total layers

### Implementation approach
- Apply a height-dependent force multiplier:
  `heightMult = 1.0 + (heightBias * (1.0 - layerProgress))`
  where `heightBias` is a configurable parameter (e.g. 0.3)
- This makes the algorithm 30% more conservative at the base and normal at the top
- Alternatively, bias tip sizes larger for low-layer supports

### Complexity
Low. Single multiplier based on layer index, trivially applied to existing force calculations.

### Pros
- Matches real-world risk profile
- Trivial to implement
- Reduces over-supporting at the top of prints where it matters least

### Cons
- The height-risk relationship varies by model geometry
- May under-support tall features that appear high but are structurally critical
- Yet another tuning parameter

### Sources
- Practical experience: most print failures occur in the first 20% of layers
- Lychee slicer documentation on "bottom support reinforcement"
- Community guides recommending heavier supports near the base

---

## Summary table

| # | Feature | Complexity | Impact | Currently modeled? |
|---|---------|-----------|--------|-------------------|
| 1 | Suction cup / vacuum effect | Medium | Very High | Yes |
| 2 | Cross-section area change rate | Low | High | Yes |
| 3 | Gravity / cumulative weight | Medium | Medium-High | Yes |
| 4 | Hydrodynamic drag (resin flow) | Medium-High | Medium | Yes |
| 5 | Thermal shrinkage / polymerization stress | Medium | High | Yes |
| 6 | Overhang angle sensitivity | Low-Medium | High | Yes |
| 7 | FEP tilt/peel kinematics | Low | Medium | Yes |
| 8 | Support-to-support structural interaction | Medium | Medium | Yes |
| 9 | Bridge and cantilever detection | Medium | High | Yes |
| 10 | Resin viscosity and drainage | Medium-High | Medium | Yes |
| 11 | Layer adhesion / delamination risk | Medium-High | Medium-High | Yes |
| 12 | Support accessibility | High | Medium | Yes |
| 13 | Surface quality-aware placement | Medium | Medium | Yes |
| 14 | Model orientation optimization | Medium-High | Very High | Yes |
| 15 | Support structure variation (tree/lattice) | High | High | No |
| 16 | Raft design and plate adhesion | Low-Medium | High | No |
| 17 | Dynamic density by print height | Low | Medium | Yes |

## Recommended implementation priority

Based on current implementation status:

1. **Support structure variation** (item 15) - still not implemented
2. **Raft design and plate adhesion** (item 16) - still not implemented
