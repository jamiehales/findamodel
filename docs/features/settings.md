---
layout: default
title: Settings
parent: Features
nav_order: 4
---

# Settings

The Settings page controls global application behaviour.

## Default raft height

**Default raft height (mm)** sets the global cutoff used when computing sans-raft hulls for printing plate placement. Models are clipped at this height from the base before the hull footprint is calculated, so the raft or brim does not inflate the footprint.

This value can be overridden per-directory using the `raftHeight` key in `findamodel.yaml`.

## Theme

Choose between the **default** light/dark theme and the **Nord** colour scheme.

## Auto support preview tuning

These settings control the lightweight auto-support preview shown on unsupported model pages. The generated supports are displayed as sphere markers only - not full scaffold geometry.

### Bed margin (mm)

Adds extra empty space around the model footprint before slice analysis begins. This helps avoid clipping supports placed near the outside edge of the model.

### Min voxel size (mm)

The minimum 2D slice-grid resolution used during coarse footprint analysis. Smaller values give more precise support placement but increase compute cost.

### Max voxel size (mm)

The upper bound for the coarse slice-grid cell size. Larger values make generation faster but reduce placement precision.

### Min layer height (mm)

The smallest vertical sampling step used while scanning upward through the model. Lower values inspect more layers and can detect smaller changes in overhang shape.

### Max layer height (mm)

The largest allowed vertical sampling step for tall models. This limits how many slices are checked during generation.

### Merge distance (mm)

If a newly proposed support point is within this distance of an existing one, it is treated as overlapping and not added again.

### Pull-force threshold

Controls how aggressively extra support points are added. Lower values result in more supports; higher values allow larger unsupported spans before another point is placed.

### Marker sphere radius (mm)

Sets the visual radius of each support marker sphere in the preview viewport. This affects the preview appearance only.

### Max supports per island

Limits how many support points may be added for one connected footprint island on a slice. This prevents runaway support placement on broad flat regions.

## Metadata dictionary

The metadata dictionary is an optional list of known values for `creator` and `collection` fields. When defined, these values appear as autocomplete suggestions in metadata editing dialogs. Dictionary values do not restrict what can be stored - they are hints only.

Add, edit, or remove dictionary entries from the Settings page.
