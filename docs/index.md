---
layout: home
title: Home
nav_order: 1
---

# FindAModel

**FindAModel** is a self-hosted 3D model library manager. It scans directories of STL, OBJ and printer-format files, automatically extracts metadata from file and folder names using a flexible rules system, and provides a web UI for browsing, tagging, and building printing plates.

---

## Key features

- **Automatic metadata extraction** — rules defined in `findamodel.yaml` files extract creator, collection, category, and other fields directly from file and folder names, without manual tagging.
- **Hierarchical configuration** — config files cascade down the directory tree so common values only need to be set once at a parent folder.
- **Hull-based printing plates** — models are placed on a canvas using 2D hull geometry (convex or concave), enabling accurate packing without model overlaps.
- **WebGL model previews** — 3D previews rendered in-browser directly from geometry.
- **Metadata filtering** — browse and filter models by creator, collection, category, material, and support status.

---

## Documentation

| Section | Description |
|---------|-------------|
| [Getting started](getting-started) | Install and run FindAModel |
| [Configuration](configuration/) | The `findamodel.yaml` configuration file |
| [Rules system](rules/) | Automatically extract metadata from paths |
| [Metadata fields](metadata/) | All supported metadata fields and valid values |
| [Features](features/) | Application features — explorer, models, plates, settings |
