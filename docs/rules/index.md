---
layout: default
title: Rules system
nav_order: 4
has_children: true
---

# Rules system
{: .no_toc }

Rules are the core mechanism for automatically extracting metadata from file and folder names. Instead of manually tagging every model, you write rules once in a `findamodel.yaml` file and FindAModel computes the correct values for every file at index time.

---

## Why use rules?

If you already organise your models into a logical folder structure - creators at the top level, then collections, then categories - rules let you extract that information without duplicating it in config files.

For example, a folder tree like:

```
Alice/
  Fantasy/
    Elves/
      elf_warrior_supported.stl
      elf_mage.stl
```

With a single root-level `findamodel.yaml`, rules can automatically extract:
- `creator` = "Alice" (from the root folder name)
- `collection` = "Fantasy" (from the second-level folder)
- `model_name` = "Elf Warrior" or "Elf Mage" (from the filename)
- `supported` = `true`/`false` (from whether "supported" appears in the filename)
- `tags` = additional per-model tags (for example, adding `supported` when filenames match)

---

## Rule types

FindAModel supports one rule type:

| Rule | Best for |
|------|----------|
| [`regex`](regex-rule) | Matching patterns anywhere in the path - folder names, full path, or filename |

---

## Rule syntax

A rule replaces the plain value for a field with a YAML object:

```yaml
field_name:
  rule: regex
  <option>: <value>
  ...
```

`rule: regex` is optional - if omitted, regex is assumed.

Rules go in `findamodel.yaml` files alongside (or instead of) plain values. They are inherited by subdirectories just like plain values - see [Inheritance](../configuration/inheritance).

---

## Evaluation order

For each model file, metadata is resolved in this order (highest priority last):

1. Inherited plain values from ancestor directories
2. Own plain value in this directory's `findamodel.yaml`
3. Inherited rule from an ancestor directory
4. Own rule in this directory's `findamodel.yaml`
5. `model_metadata` per-file override ← highest priority

Rules override plain values of equal or lower specificity. A plain value in the same directory as a rule means the plain value is ignored - the rule takes effect instead.

---

## Next steps

- [Regex rule](regex-rule) - match and extract values from path segments
