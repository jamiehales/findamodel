---
layout: default
title: Filename rule
parent: Rules system
nav_order: 1
---

# Filename rule
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Overview

The `filename` rule sets a field to the name of the model file, with automatic title-casing applied. It is most commonly used for `model_name`.

```yaml
model_name:
  rule: filename
```

---

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `include_extension` | boolean | `false` | When `true`, includes the file extension (e.g. `.stl`) in the returned value |

---

## Behaviour

1. Takes the file's name (with or without extension, depending on `include_extension`).
2. Applies **title-casing**: the result is lowercased then each word's first letter is capitalised.

Underscores and hyphens are treated as word separators for title-casing purposes.

---

## Examples

### Basic - model name from filename

```yaml
model_name:
  rule: filename
```

| File | Result |
|------|--------|
| `elf_warrior.stl` | `Elf Warrior` |
| `dragon-head.stl` | `Dragon-Head` |
| `SPACESHIP.stl` | `Spaceship` |
| `my model v2.stl` | `My Model V2` |

### Including the extension

```yaml
model_name:
  rule: filename
  include_extension: true
```

| File | Result |
|------|--------|
| `elf_warrior.stl` | `Elf Warrior.Stl` |

{: .note }
Including the extension is uncommon - most of the time you want the name without it.

---

## Using `include_extension: false` (default)

`include_extension` defaults to `false`, so these two are equivalent:

```yaml
model_name:
  rule: filename

model_name:
  rule: filename
  include_extension: false
```

---

## Common use cases

The filename rule is almost exclusively used for `model_name`. For extracting metadata from parts of the path (collection names, support status, etc.), use the [regex rule](regex-rule) instead.
