---
layout: default
title: findamodel.yaml format
parent: Configuration
nav_order: 1
---

# findamodel.yaml format
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Overview

Place a `findamodel.yaml` file in any directory to configure metadata for models in that folder and all its subdirectories.

The filename must be exactly **`findamodel.yaml`** (lowercase).

---

## Plain value fields

Set a field to a fixed string or boolean. These values apply to every model in the directory (and are inherited by subdirectories unless overridden).

```yaml
creator: "Alice"
collection: "Fantasy Heroes"
subcollection: "Elves"
category: "miniature"
type: "whole"
material: "resin"
supported: true
raftHeight: 2.5
model_name: "Elf Warrior"
```

### Supported fields

| Key | Type | Description |
|-----|------|-------------|
| `creator` | string | Person or organization that made the model |
| `collection` | string | Top-level grouping (e.g. a Kickstarter set or product range) |
| `subcollection` | string | Sub-grouping within a collection |
| `category` | enum | Type of model — see [Metadata fields](../metadata/) for valid values |
| `type` | enum | Whether the file is a complete model or a single part |
| `material` | enum | Print technology the model is designed for |
| `supported` | boolean | Whether the file already includes print supports |
| `raftHeight` | float (mm) | Height at which to cut the model for the sans-raft hull used on printing plates |
| `model_name` | string | Display name for the model |

See [Metadata fields](../metadata/) for the full list of valid enum values.

---

## Rule fields

Instead of a fixed value, a field can use a **rule** to compute its value dynamically from each file's path. Set the field key to a YAML object with a `rule:` property.

```yaml
creator:
  rule: regex
  source: folder
  expression: "^([^/]+)"

model_name:
  rule: filename
  include_extension: false
```

Rules are evaluated per file at index time. See the [Rules system](../rules/) section for a complete reference.

---

## Per-file metadata overrides

The `model_metadata` section provides per-file overrides that take the highest priority — they override both plain values and rules.

```yaml
model_metadata:
  "dragon.stl":
    name: "Custom Dragon"
    part_name: "Head"
    creator: "Bob"
    collection: "Dragons"
    category: "bust"
    supported: false
  "warrior.stl":
    name: "Elite Warrior"
```

The key is the **filename only** (not the full path). The following per-file fields are supported:

| Key | Description |
|-----|-------------|
| `name` | Display name for this specific file |
| `part_name` | Part identifier (e.g. "Left arm", "Head") |
| `creator` | Override creator for this file |
| `collection` | Override collection |
| `subcollection` | Override subcollection |
| `category` | Override category |
| `type` | Override type |
| `material` | Override material |
| `supported` | Override support status |

---

## Combining plain values, rules and per-file overrides

All three mechanisms can coexist in a single file. Priority order (highest wins):

1. **`model_metadata` override** for the specific file
2. **Rule** result evaluated for the file
3. **Plain value** set in this file or inherited from a parent

```yaml
# Plain value: used if no rule matches and no per-file override
creator: "Alice"

# Rule: computed per file; overrides the plain value if it returns a result
category:
  rule: regex
  source: filename
  values:
    Miniature: "mini|figure|warrior"
    Bust: "bust|head"

# Per-file override: highest priority; overrides rule and plain value
model_metadata:
  "special_bust.stl":
    category: "bust"
    creator: "Charlie"
```

---

## Full example

```yaml
# Who made these models
creator: "Alice"

# All models here are resin prints
material: "resin"

# Determine collection from the immediate parent folder name
collection:
  rule: regex
  source: folder
  expression: "([^/]+)$"

# Determine support status from the filename
supported:
  rule: regex
  expression: "supported"

# Extract model name from filename
model_name:
  rule: filename
  include_extension: false

# Override specific files
model_metadata:
  "concept_dragon.stl":
    name: "Concept Dragon (WIP)"
    creator: "Bob"
  "dragon_head_supported.stl":
    part_name: "Head"
```
