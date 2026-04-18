---
layout: default
title: Auto-Support V2 Optimization Plan
nav_order: 9
---

# Auto-Support V2 Optimization Plan

## Goal

Reduce method 2 runtime and memory cost at small voxel sizes (especially 0.25 mm and 0.1 mm)
without materially reducing support quality.

Current behavior in `backend/Services/AutoSupportGenerationV2Service.cs` uses a uniform voxel size
for the entire model volume. This creates near-cubic cost growth as voxel size shrinks.

## Why this is needed

For a fixed model volume, voxel count scales approximately as:

- `work ~ 1 / voxelSize^3`

Relative to 0.5 mm:

- 0.25 mm => ~8x cells
- 0.10 mm => ~125x cells

Even if not every code path is purely cubic, this is the right first-order pressure model for CPU,
RAM, and allocation churn.

## Scope

Optimize method 2 generation pipeline only.

In scope:

- voxelization / occupancy generation
- island detection and connectivity handling
- force distribution and reinforcement loop
- settings and metrics needed to control and validate optimization

Out of scope:

- changing method 1 algorithm behavior
- changing final support geometry format
- replacing existing API endpoints

## Existing pipeline summary

In `AutoSupportGenerationV2Service.GenerateSupportPreview`:

1. Resolve tuning from app config.
2. Compute single global `voxelSizeMm`.
3. Render all layers as full bitmaps with `OrthographicProjectionSliceBitmapGenerator`.
4. Detect 2D islands per layer and track 3D connectivity by previous-layer IDs.
5. Maintain cumulative per-island force state.
6. Place initial supports and run reinforcement loop based on pull force and torque.
7. Emit support markers and mesh.

Primary hotspots at small voxel size:

- full-grid layer bitmaps (`gridW * gridD * layerCount`)
- per-island force distribution repeatedly scanning many cells
- repeated nearest-support assignment for each reinforcement iteration

## Target architecture

Use a two-stage, region-adaptive approach:

1. Coarse global pass:
- Run full model at coarse voxel size (default 0.5 mm)
- Identify risk regions / islands likely to need additional precision

2. Selective refinement pass:
- Reprocess only marked regions at finer size (e.g. 0.25 or 0.1 mm)
- Keep coarse results outside refined regions
- Recompute forces only where refined data changed

This keeps expensive resolution localized to meaningful geometry.

## Phase plan

### Phase 1 - Instrumentation and baseline

Add telemetry before changing algorithm:

- total runtime
- layer rendering time
- island detection time
- force evaluation time
- supports added / reinforced count
- estimated active voxel count
- peak managed allocations

Implementation touchpoints:

- `backend/Services/AutoSupportGenerationV2Service.cs`
- `backend/Controllers/ModelsController.cs` (optional response diagnostics exposure)

Expected output:

- stable benchmark table for voxel sizes: 0.5, 0.25, 0.1

### Phase 2 - Coarse pass + candidate detection

Run a coarse pass and mark candidate regions for refinement.

Candidate criteria (any match):

- high unsupported area/volume above threshold
- supports near capacity (pull or torque margin below threshold)
- thin geometry indicators (rapid occupancy changes across adjacent layers)
- high reinforcement churn in coarse loop

Represent candidate as axis-aligned region box:

- `(minX, maxX, minY, maxY, minZ, maxZ)` with expansion margin

Expected output:

- list of refinement regions with per-region priority

### Phase 3 - Regional refinement

For each candidate region:

- regenerate occupancy at fine voxel size inside region bounds only
- recompute layer islands and 3D connectivity for region-local cells
- map refined islands back to parent coarse island identity

Merge strategy:

- outside region: coarse state remains authoritative
- inside region: refined state overrides coarse state
- boundary overlap: blend by region precedence and deterministic tie-breaker

Deterministic tie-breaker:

- prefer highest resolution cell data
- if same resolution, prefer smallest support-index distance

### Phase 4 - Incremental force recomputation

Avoid full recompute after each support placement.

Maintain region-local force cache keyed by:

- island ID
- support set hash (support positions + sizes)
- resolution level

On support add/upgrade:

- invalidate only affected island-region entries
- recompute local force buckets
- update global worst-support candidate from local deltas

### Phase 5 - Spatial acceleration for nearest-support assignment

Replace repeated brute-force support distance scans.

Option A (recommended first):

- uniform spatial hash grid
- cell width = `MaxSupportDistanceMm`
- candidate supports from neighboring bins only

Option B:

- k-d tree rebuilt per reinforcement iteration if support set changed

Use Option A first for simpler incremental updates.

### Phase 6 - Optional adaptive octree

After two-stage approach is stable, evaluate full octree occupancy store.

- leaf size constrained by `MinVoxelSizeMm`
- split nodes only where occupancy/curvature changes exceed thresholds
- keep flattened layer projection interface for compatibility

This phase is optional and higher complexity.

## Data structures

### New types (proposed)

- `ResolutionLevel` enum (`Coarse`, `Fine`)
- `RefinementRegion` record
- `RegionVoxelGrid` (sparse occupancy chunks)
- `IslandRegionKey` (`IslandId`, `RegionId`, `ResolutionLevel`)
- `ForceCacheEntry`
- `SupportSpatialIndex`

### Storage approach

Prefer sparse chunked storage over dense arrays at fine levels:

- chunk dimensions: `32 x 32 x 32` voxels
- dictionary keyed by chunk coordinate
- each chunk uses compact bitset for occupancy + optional metadata arrays

Rationale:

- avoids allocating empty space
- improves cache locality for active geometry
- limits peak memory spikes

## Configuration additions

Add method 2 optimization settings to app config.

Proposed fields:

- `AutoSupportV2OptimizationEnabled` (bool, default `true`)
- `AutoSupportV2CoarseVoxelSizeMm` (float, default `0.5`)
- `AutoSupportV2FineVoxelSizeMm` (float, default current method setting)
- `AutoSupportV2RefinementMarginMm` (float, default `2.0`)
- `AutoSupportV2RefinementMaxRegions` (int, default `12`)
- `AutoSupportV2RiskForceMarginRatio` (float, default `0.2`)
- `AutoSupportV2MinRegionVolumeMm3` (float, default `8.0`)

Validation constraints:

- finite and positive for all lengths/volumes
- `FineVoxelSizeMm <= CoarseVoxelSizeMm`
- `RefinementMaxRegions` in safe range (e.g. `1..128`)

Backend files impacted:

- `backend/Data/Entities/AppConfig.cs`
- `backend/Models/SettingsDtos.cs`
- `backend/Services/AppConfigService.cs`
- `backend/Data/ModelCacheContext.cs`
- EF migration in `backend/Migrations/`

Frontend files impacted:

- `frontend/src/lib/api/explorer.ts`
- `frontend/src/pages/SettingsPage.tsx`

## Pseudocode

```text
GenerateSupportPreviewOptimized(geometry):
  tuning = ResolveTuning()

  if !tuning.OptimizationEnabled:
    return GenerateSupportPreviewUniform(geometry, tuning.FineVoxelSizeMm)

  coarseResult = RunUniformPass(geometry, voxel=tuning.CoarseVoxelSizeMm)

  candidateRegions = DetectRefinementRegions(
    coarseResult,
    margin=tuning.RefinementMarginMm,
    maxRegions=tuning.RefinementMaxRegions,
    riskForceMargin=tuning.RiskForceMarginRatio,
    minVolume=tuning.MinRegionVolumeMm3)

  if candidateRegions empty:
    return coarseResult

  refinedState = coarseResult.State
  spatialIndex = BuildSupportSpatialIndex(refinedState.SupportPoints)

  for region in candidateRegions:
    regionFine = RunRegionalPass(
      geometry,
      region,
      voxel=tuning.FineVoxelSizeMm,
      inheritedState=refinedState,
      spatialIndex=spatialIndex)

    refinedState = MergeRegionalResult(refinedState, regionFine)
    UpdateForceCacheAndWorstSupport(refinedState, region)

  return BuildFinalResult(refinedState)
```

## Implementation sequence (commit-level)

1. Add profiling + benchmark harness for method 2.
2. Introduce coarse pass API and candidate detection.
3. Implement regional fine pass and state merge.
4. Add incremental force cache invalidation and recomputation.
5. Add support spatial hash index.
6. Add config fields + migration + settings UI.
7. Tune defaults with benchmark data.

## Benchmark protocol

Use representative models in three groups:

- small simple (single figurine)
- medium complex (multiple thin features)
- large complex (dense detail + overhangs)

For each model:

- run 5 iterations per setting
- capture median and p95 runtime
- capture peak memory
- compare support count and support location deltas

Compare settings:

- baseline uniform 0.5
- baseline uniform 0.25
- baseline uniform 0.1
- optimized coarse 0.5 / fine 0.25
- optimized coarse 0.5 / fine 0.1

Acceptance targets:

- support count delta within +/-10% of uniform fine run
- no increase in clearly unsupported islands over configured threshold
- runtime reduction:
  - at least 2x vs uniform 0.25
  - at least 5x vs uniform 0.1
- memory reduction at least 2x vs uniform fine runs

## Correctness and regression tests

Add tests in `backend.Tests/`:

- coarse-only path matches previous behavior when no candidates
- refinement region merge determinism
- force cache invalidation correctness after support insert/upgrade
- spatial index nearest-support parity with brute force
- settings validation boundaries for new optimization fields

Also add integration tests:

- method 2 job with optimization on/off
- stability of output envelope shape and metadata

## Observability

Log one structured summary per job:

- method version
- optimization enabled
- coarse/fine voxel sizes
- region count
- runtime by stage
- support count
- peak memory estimate

This enables post-deploy tuning without debug builds.

## Risks and mitigations

Risk: Region merging creates discontinuities at boundaries.

Mitigation:

- overlap margin
- deterministic precedence by resolution
- boundary reconciliation pass

Risk: Over-refinement causes many small regions.

Mitigation:

- cap max regions
- merge nearby boxes
- ignore below min volume threshold

Risk: Complex cache invalidation bugs.

Mitigation:

- start with conservative invalidation by island-region
- parity test against full recompute

## Rollout strategy

1. Ship instrumentation first.
2. Ship optimization behind config flag default off.
3. Enable by default for development after benchmark validation.
4. Enable by default generally once parity and perf targets are stable.

## Quick checklist for implementation

- [ ] Add metrics in V2 service
- [ ] Add coarse pass candidate detection
- [ ] Add regional fine pass
- [ ] Add merge + deterministic boundary logic
- [ ] Add incremental force cache
- [ ] Add spatial index
- [ ] Add app config fields and validation
- [ ] Add EF migration
- [ ] Add settings UI controls
- [ ] Add backend unit + integration tests
- [ ] Run `dotnet test backend.Tests/findamodel.Tests.csproj`
- [ ] Document benchmark results
