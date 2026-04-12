---
layout: default
title: LLM Configuration
nav_order: 4
---

# LLM Configuration
{: .no_toc }

FindAModel supports local tag generation through two providers:

- `internal` (default): runs a local model using LLamaSharp
- `ollama`: calls an external Ollama endpoint over HTTP

---

## Provider selection

Provider selection is stored in app settings (`/api/settings/config`) using:

- `tagGenerationProvider`: `internal` or `ollama`

Default behavior:

- default provider is `internal`
- if you set `tagGenerationProvider` to `ollama`, FindAModel uses Ollama instead

---

## Internal provider model download and cache

The internal provider auto-downloads a GGUF model from Hugging Face on startup (or first use if warmup fails), then caches it locally.

Appsettings keys:

```json
{
  "LocalLlm": {
    "Internal": {
      "ModelUrl": "https://huggingface.co/.../model.gguf",
      "ModelSha256": "",
      "CachePath": ""
    }
  }
}
```

- `ModelUrl`: direct download URL to a GGUF file
- `ModelSha256`: optional checksum validation (hex string)
- `CachePath`: optional absolute/relative cache directory

If `CachePath` is empty, cache defaults to:

- `{Configuration:DataPath}/cache/llm`

---

## GPU behavior (internal provider)

Internal inference is GPU-first by default and falls back to CPU automatically if GPU initialization fails.

Appsettings keys:

```json
{
  "LocalLlm": {
    "Internal": {
      "UseGpu": true,
      "GpuLayerCount": 35
    }
  }
}
```

- `UseGpu`: enables GPU offload when `true`
- `GpuLayerCount`: number of model layers to offload to GPU

Recommendations:

- keep `UseGpu: true` for best performance
- lower `GpuLayerCount` if VRAM is limited
- set `UseGpu: false` for deterministic CPU-only behavior

---

## Tag generation app config fields

These values are managed through the settings API/UI and persisted in the database:

- `tagGenerationEnabled`
- `tagGenerationProvider`
- `tagGenerationEndpoint`
- `tagGenerationModel`
- `tagGenerationTimeoutMs`
- `tagGenerationAutoApply`
- `tagGenerationMaxTags`
- `tagGenerationMinConfidence`

Notes:

- `tagGenerationEnabled` defaults to `true`
- non-schema tags are rejected
- with `tagGenerationAutoApply: true`, accepted generated tags are merged into effective model tags
- `tagGenerationMinConfidence` is the minimum score required for a generated tag to be kept

---

## Ollama configuration

When using `tagGenerationProvider: ollama`, configure:

- `tagGenerationEndpoint` (for example `http://localhost:11434`)
- `tagGenerationModel` (for example `qwen2.5vl:7b`)

The current tagging pipeline sends preview-image context when available and metadata context always.
