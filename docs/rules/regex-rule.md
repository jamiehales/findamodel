---
layout: default
title: Regex rule
parent: Rules system
nav_order: 2
---

# Regex rule
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Overview

The `regex` rule evaluates a regular expression against part of the file's path and returns a value. It is the most flexible rule type and can be used for any metadata field.

```yaml
creator:
  rule: regex
  source: folder
  expression: "^([^/]+)"
```

---

## Options

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `source` | string | No | Which part of the path to match against. One of `full_path`, `folder`, `filename`. Defaults to `full_path`. |
| `expression` | string | Yes* | A regex pattern or sed-style substitution expression (`s\|pattern\|replacement\|flags`). |
| `values` | map | Yes* | For enum fields: a map of enum value → regex pattern. Mutually exclusive with `expression`. |

\* Either `expression` or `values` must be present (not both).

---

## Source modes

The `source` option controls what string the regex is applied to.

| Value | Input string |
|-------|-------------|
| `full_path` | The file's full path, normalised to forward slashes |
| `folder` | The directory portion of the path (everything except the filename) |
| `filename` | Just the filename (including extension) |

### Examples

For a file at `/models/Alice/Fantasy/elf_warrior.stl`:

| Source | Input to regex |
|--------|---------------|
| `full_path` | `/models/Alice/Fantasy/elf_warrior.stl` |
| `folder` | `/models/Alice/Fantasy` |
| `filename` | `elf_warrior.stl` |

---

## Expression modes

### Plain regex - extract a value

If the expression contains a **capturing group**, the content of the first group is returned. If there are no groups, the full match is returned. If there is no match, the rule returns no value (the field is left unset).

```yaml
# Extract the first path segment after /models/
creator:
  rule: regex
  source: folder
  expression: "^/models/([^/]+)"
```

For `/models/Alice/Fantasy/elf_warrior.stl` this returns `"Alice"`.

```yaml
# Extract the word "supported" or "unsupported" from the filename
collection:
  rule: regex
  source: filename
  expression: "(supported|unsupported)"
```

Matching is **case-insensitive** by default.

---

### Boolean fields - match or no match

When the rule is applied to a `boolean` field (`supported`), the expression acts as a presence check:
- If the regex **matches** → field is set to `true`
- If the regex does **not** match → field is set to `false`

```yaml
supported:
  rule: regex
  expression: "supported"
```

| File | Result |
|------|--------|
| `elf_warrior_supported.stl` | `true` |
| `elf_warrior.stl` | `false` |
| `SUPPORTED_dragon.stl` | `true` (case-insensitive) |

---

### Enum fields - values map

For enum fields (`category`, `type`, `material`), use the `values` map instead of `expression`. The map keys are the enum values to return; the map values are regexes to test against the input. The first matching key is returned.

```yaml
category:
  rule: regex
  source: filename
  values:
    Miniature: "mini|figure|warrior|soldier"
    Bust: "bust|head|portrait"
```

| File | Result |
|------|--------|
| `elf_warrior.stl` | `Miniature` |
| `dragon_bust.stl` | `Bust` |
| `spaceship.stl` | *(not set - no match)* |

{: .note }
The keys in the `values` map must exactly match valid values for the target field (case-sensitive). See [Metadata fields](../metadata/) for the valid enum values.

{: .note }
`values` and `expression` are mutually exclusive. If both are provided, `values` takes precedence for enum fields.

---

### Sed-style substitution

The `expression` option also accepts a **sed-style substitution** of the form:

```
s<delim><pattern><delim><replacement><delim>[flags]
```

The delimiter can be any character (commonly `|` or `/`). Backreferences use `\1`, `\2`, etc.

This is useful for transforming a path segment into a cleaned-up string.

```yaml
collection:
  rule: regex
  source: folder
  expression: "s|.*/([^/]+)/[^/]+$|\1|"
```

For `/models/Alice/Fantasy/Elves/elf.stl` (folder = `/models/Alice/Fantasy/Elves`), captures `Fantasy` (the parent of `Elves`).

#### Supported flags

| Flag | Meaning |
|------|---------|
| `i` | Case-insensitive (already on by default) |
| `m` | Multiline mode |
| `s` | Singleline mode (`.` matches newlines) |
| `x` | Ignore whitespace in pattern |

---

## Full examples

### Extract creator from top-level folder

```yaml
creator:
  rule: regex
  source: folder
  expression: "^/?([^/]+)"
```

`/models/Alice/Fantasy/elf.stl` → `"Alice"`

---

### Extract collection from immediate parent folder

```yaml
collection:
  rule: regex
  source: folder
  expression: "([^/]+)$"
```

`/models/Alice/Fantasy/Elves/elf.stl` (folder = `/models/Alice/Fantasy/Elves`) → `"Elves"`

To capture the grandparent instead:

```yaml
collection:
  rule: regex
  source: folder
  expression: "([^/]+)/[^/]+$"
```

→ `"Fantasy"`

---

### Detect support status from filename

```yaml
supported:
  rule: regex
  source: filename
  expression: "_sup(ported)?(_|\\.|$)"
```

Matches `elf_supported.stl` and `elf_sup.stl` but not `unsupported.stl`.

---

### Categorise by filename keywords

```yaml
category:
  rule: regex
  source: filename
  values:
    Miniature: "mini|figure|soldier|warrior|knight|ranger|rogue|paladin"
    Bust: "bust|portrait|head"
```

---

### Extract and transform: title-case a folder segment

Use a sed expression to clean up a folder name:

```yaml
collection:
  rule: regex
  source: folder
  expression: "s|_| |g"
```

Converts `_` to spaces in the folder path. Combined with a capture group:

```yaml
collection:
  rule: regex
  source: folder
  expression: "s|.*/([^/]+)$|\1|"
```

Extracts (but does not title-case) the immediate parent folder.

---

## Combining multiple rules

Each field can have only one rule. To handle complex scenarios, structure your directory tree so that different `findamodel.yaml` files at different levels apply different rules, taking advantage of [inheritance](../configuration/inheritance).
