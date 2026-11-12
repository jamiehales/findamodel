---
layout: default
title: Models
parent: Features
nav_order: 2
---

# Models

The Models page shows every indexed model in one flat, searchable, filterable list.

## Filtering

Use the filter panel to narrow results by:
- **Creator**
- **Collection**
- **Category** (Miniature, Bust, Uncategorized)
- **Type** (Whole, Part)
- **Material** (FDM, Resin, Any)
- **Supported** (has supports / no supports)

## Sorting

Results can be sorted by name, creator, collection, or date indexed.

## Model detail

Click any model card to open the detail view, which shows:
- A 3D WebGL preview of the model
- All resolved metadata fields
- The file size and dimensions
- Options to add the model to a printing plate

## Downloads

The download menu on a model offers:
- **Original**: the file as it is in the library.
- **DragonFruit .voxl** (pre-supported models): the model without its supports, with the supports read from the file and rebuilt as DragonFruit supports, which DragonFruit builds itself.
- **DragonFruit .voxl (remeshed)**: the same, with the model mesh repaired so it prints cleanly: holes closed, overlapping and self-crossing parts merged, faces turned the right way, specks and zero-thickness sheets dropped, and sealed voids (closed cavities with no drain, which trap resin) filled. Hollows that drain through a real opening stay hollow. The supports are the same DragonFruit supports; the original support mesh is never included. The surface is kept exactly except at the repaired spots, which are remeshed on a 0.05 mm grid. The first download of a model takes from a second to a few minutes (models that are open all over are rebuilt whole); later downloads come from the cache.

