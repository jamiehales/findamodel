---
layout: default
title: Configuration
nav_order: 3
has_children: true
---

# Configuration
{: .no_toc }

FindAModel is configured through `findamodel.yaml` files placed directly inside your model directories. These files control metadata values and rules for the directory and all its subdirectories.

---

## How it works

When FindAModel indexes your library it:

1. Walks the directory tree recursively.
2. Reads any `findamodel.yaml` it finds in each folder.
3. **Inherits** configuration from parent directories to child directories — values set higher in the tree apply everywhere below unless overridden.
4. Evaluates any [rules](../rules/) defined in the config to compute metadata for each model file.

---

## Quick example

Given this directory tree:

```
models/
├── findamodel.yaml          ← sets creator: "Alice"
├── Fantasy/
│   ├── findamodel.yaml      ← sets collection: "Fantasy"
│   └── Elves/
│       ├── findamodel.yaml  ← model_name rule: filename
│       ├── elf_warrior.stl
│       └── elf_mage.stl
└── SciFi/
    └── spaceship.stl        ← inherits creator: "Alice" from root
```

`elf_warrior.stl` resolves to:
- `creator` → "Alice" (from root)
- `collection` → "Fantasy" (from Fantasy/)
- `model_name` → "Elf Warrior" (from filename rule in Elves/)

`spaceship.stl` resolves to:
- `creator` → "Alice" (from root)
- No collection set (SciFi/ has no findamodel.yaml)

---

## Next steps

- [findamodel.yaml format](findamodel-yaml) — all supported keys and syntax
- [Inheritance](inheritance) — detailed rules for how values cascade down the tree
- [LLM configuration](llm) — internal LLamaSharp and Ollama runtime settings
