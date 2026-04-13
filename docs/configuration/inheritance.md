---
layout: default
title: Inheritance
parent: Configuration
nav_order: 2
---

# Configuration inheritance
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Overview

`findamodel.yaml` files form a hierarchy that mirrors your directory tree. When a field is not set in a directory's own config, FindAModel walks up the tree to the nearest ancestor that has a value for that field - this is the **resolved** value used for all models in that directory.

This means you only need to set shared values once, high in the tree, rather than repeating them in every subdirectory.

---

## Inheritance rules

- **Closest non-null value wins.** For each metadata field, FindAModel walks from the directory up toward the root and uses the first non-null value it finds.
- **The child always wins over the parent.** A value set in a subdirectory always takes priority over the same field in a parent directory.
- **Rules inherit too.** If a directory has a rule for a field (instead of a plain value), and a child directory has neither a plain value nor its own rule for that field, the child inherits the parent's rule. The rule is then evaluated using the *child's* file paths.
- **Plain values take priority over inherited rules.** If an ancestor has a rule but the current directory (or a closer ancestor) has a plain value, the plain value wins.

---

## Example

```
models/
├── findamodel.yaml       creator: "Alice"
│                         material: "resin"
│
├── Fantasy/
│   ├── findamodel.yaml   collection: "Fantasy"
│   │
│   ├── Elves/
│   │   ├── findamodel.yaml   model_name: { source: filename, expression: "^(.*)\\.[^./]+$" }
│   │   ├── elf_warrior.stl
│   │   └── elf_mage.stl
│   │
│   └── Dwarves/
│       └── dwarf_king.stl    (no findamodel.yaml)
│
└── SciFi/
    ├── findamodel.yaml   collection: "SciFi"
    │                     creator: "Bob"   ← overrides root "Alice"
    └── spaceship.stl
```

Resolved values:

| File | creator | collection | material | model_name |
|------|---------|------------|----------|------------|
| `Fantasy/Elves/elf_warrior.stl` | Alice | Fantasy | resin | elf_warrior |
| `Fantasy/Elves/elf_mage.stl` | Alice | Fantasy | resin | elf_mage |
| `Fantasy/Dwarves/dwarf_king.stl` | Alice | Fantasy | resin | *(not set)* |
| `SciFi/spaceship.stl` | Bob | SciFi | resin | *(not set)* |

Key observations:
- `material: "resin"` is set once at the root and inherited everywhere.
- `creator: "Bob"` in `SciFi/` overrides the root `creator: "Alice"` for everything under `SciFi/`.
- The `model_name` regex rule in `Elves/` applies only to files in that directory.
- `Dwarves/` has no `findamodel.yaml`, so it inherits `collection: "Fantasy"` and `creator: "Alice"` but has no `model_name` rule.

---

## Rule inheritance

Rules inherit like plain values - the closest ancestor rule wins. The rule is then evaluated against each file in the child directory using that file's own path.

```
models/
├── findamodel.yaml
│     model_name:
│       source: filename
│       expression: "^(.*)\\.[^./]+$"     ← inherited by all subdirs with no own model_name
│
├── Fantasy/
│   └── dragon.stl         → model_name evaluates to "dragon"
│
└── SciFi/
    ├── findamodel.yaml
    │     model_name: "Override"    ← plain value; wins over inherited rule
    └── spaceship.stl               → model_name is "Override"
```

---

## When inheritance does not apply

- **`model_metadata` overrides** are per-file and directory-local. They are not inherited by subdirectories.
- **`raftHeight`** is a per-directory setting that controls the hull clipping plane; it is inherited but affects geometry computation rather than displayed metadata.
