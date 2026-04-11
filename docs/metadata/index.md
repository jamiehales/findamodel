---
layout: default
title: Metadata fields
nav_order: 5
---

# Metadata fields
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Overview

Every model in FindAModel has a set of metadata fields that describe it. These fields are populated by a combination of plain values, rules, and per-file overrides defined in `findamodel.yaml` files.

---

## Field reference

### `creator`

| | |
|---|---|
| **Type** | String |
| **Sources** | Plain value, regex rule, per-file override |
| **Description** | The person or organisation who designed the model. Typically the artist name or studio. |

```yaml
creator: "Alice"
```

---

### `collection`

| | |
|---|---|
| **Type** | String |
| **Sources** | Plain value, regex rule, per-file override |
| **Description** | A named grouping of related models, such as a Kickstarter set, product range or theme. |

```yaml
collection: "Fantasy Heroes"
```

---

### `subcollection`

| | |
|---|---|
| **Type** | String |
| **Sources** | Plain value, regex rule, per-file override |
| **Description** | A sub-grouping within a collection. Useful for very large releases that have multiple sub-sets. |

```yaml
subcollection: "High Elves"
```

---

### `category`

| | |
|---|---|
| **Type** | Enum |
| **Sources** | Plain value, regex rule (`values` map), per-file override |
| **Valid values** | `Miniature`, `Bust`, `Uncategorized` |
| **Description** | The broad category of the model. |

```yaml
category: miniature
```

Valid values (case-insensitive in config, stored as shown):

| Value | Description |
|-------|-------------|
| `Miniature` | A full-body tabletop miniature or figure |
| `Bust` | A chest/head portrait model |
| `Uncategorized` | No category assigned (default) |

---

### `type`

| | |
|---|---|
| **Type** | Enum |
| **Sources** | Plain value, regex rule (`values` map), per-file override |
| **Valid values** | `Whole`, `Part` |
| **Description** | Whether the file represents a complete model or a single component part. |

```yaml
type: whole
```

| Value | Description |
|-------|-------------|
| `Whole` | A complete, standalone model |
| `Part` | A single part of a multi-part model |

---

### `material`

| | |
|---|---|
| **Type** | Enum |
| **Sources** | Plain value, regex rule (`values` map), per-file override |
| **Valid values** | `FDM`, `Resin`, `Any` |
| **Description** | The print technology the file is designed for. |

```yaml
material: resin
```

| Value | Description |
|-------|-------------|
| `FDM` | Fused deposition modelling (filament printer) |
| `Resin` | MSLA/DLP resin printer |
| `Any` | Not technology-specific |

---

### `supported`

| | |
|---|---|
| **Type** | Boolean |
| **Sources** | Plain value, regex rule (presence check), per-file override |
| **Description** | Whether the file already includes pre-generated print supports. |

```yaml
supported: true
```

When using a regex rule, any match sets the value to `true` and no match sets it to `false`.

```yaml
supported:
  rule: regex
  expression: "supported"
```

---

### `model_name`

| | |
|---|---|
| **Type** | String |
| **Sources** | Plain value, filename rule, regex rule, per-file override |
| **Description** | The display name for the model shown in the UI. If not set, the raw filename (without extension) is shown. |

```yaml
model_name: "Elf Warrior"
```

Most commonly populated with the [filename rule](rules/filename-rule):

```yaml
model_name:
  rule: filename
```

---

### `part_name`

| | |
|---|---|
| **Type** | String |
| **Sources** | Per-file override only |
| **Description** | A label for the specific part represented by this file (e.g. "Left arm", "Head"). Only settable via `model_metadata`. |

```yaml
model_metadata:
  "elf_left_arm.stl":
    part_name: "Left Arm"
```

---

## Metadata dictionary

FindAModel supports a **metadata dictionary** — a configurable list of known/expected values for string fields like `creator` and `collection`. This is used in the UI for autocomplete suggestions but does not restrict the values that can be stored.

Configure the dictionary on the **Settings** page in the application.
