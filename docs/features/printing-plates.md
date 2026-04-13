---
layout: default
title: Printing plates
parent: Features
nav_order: 3
---

# Printing plates
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Overview

The printing plate feature lets you arrange 3D models on a virtual build plate for batch printing. Models are represented by their 2D hull footprint, enabling compact packing without overlaps.

---

## Plates list

The **Plates** section shows all your saved printing plates. From here you can:
- Create a new plate
- Rename or delete existing plates
- Open a plate for editing

---

## Plate canvas

Opening a plate shows a top-down view of the build plate as a canvas.

### Adding models

Models can be added to the plate from the model detail page, or by dragging from the model library panel.

### Placement and rotation

- **Click** a model on the canvas to select it.
- **Drag** to move it to a new position.
- **Rotate** using the rotation handle or the rotation input field.

### Hull modes

The 2D footprint of each model is computed from its geometry:

| Mode | Description |
|------|-------------|
| **Convex** | Fast, simplified outline - good for most models |
| **Concave** | Detailed boundary that follows the model's silhouette - allows tighter packing for complex shapes |

The active hull mode is set per-plate in plate settings.

### Raft clearance

The `raftHeight` setting clips the bottom of the model before hull computation - models with a raft or brim base can have their hulls computed from just above the raft, giving a more accurate footprint for placement.

Set `raftHeight` globally in [Settings](settings) or per-directory in `findamodel.yaml`.

---

## Plate settings

| Setting | Description |
|---------|-------------|
| **Hull mode** | Convex or concave hull for collision/placement |
| **Spawn position** | Where newly added models appear (centre, edge, etc.) |
