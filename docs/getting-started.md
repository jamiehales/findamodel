---
layout: default
title: Getting started
nav_order: 2
---

# Getting started
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Requirements

- **Docker** (recommended) - the easiest way to run FindAModel.
- Or: .NET 8 SDK and Node.js 20+ for local development.

For desktop-specific prerequisites and build flow, see [Desktop > Getting started (desktop)](desktop/getting-started).

---

## Running with Docker

Pull and run the pre-built image from GitHub Container Registry:

```bash
docker run -d \
  --name findamodel \
  -e PUID=1000 \
  -e PGID=1000 \
  -e UMASK=022 \
  -p 5000:8080 \
  -v /path/to/your/models:/models:ro \
  -v /path/to/data:/data \
  ghcr.io/<owner>/findamodel:latest
```

Then open [http://localhost:5000](http://localhost:5000) in your browser.

If you want NVIDIA GPU acceleration for local LLM inference and OpenGL preview rendering, add:

```bash
--gpus all \
-e NVIDIA_VISIBLE_DEVICES=all \
-e NVIDIA_DRIVER_CAPABILITIES=compute,utility,graphics
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MODELS_PATH` | `/models` | Path to your models directory (can be read-only) |
| `DATA_PATH` | `/data` | Path for the database and cache files |
| `PUID` | `1000` | Run the app process as this user id (LinuxServer style) |
| `PGID` | `1000` | Run the app process as this group id (LinuxServer style) |
| `UMASK` | `022` | Umask applied before starting the app process |
| `UMASK_SET` | _(deprecated)_ | Legacy alias for `UMASK` for LinuxServer compatibility |
| `NVIDIA_VISIBLE_DEVICES` | `all` | NVIDIA device visibility for GPU inference and preview rendering (`none` to force CPU-only) |
| `NVIDIA_DRIVER_CAPABILITIES` | `compute,utility,graphics` | Required NVIDIA driver capabilities for CUDA inference and OpenGL preview rendering |
| `ASPNETCORE_URLS` | `http://+:8080` | Bind address |

### docker-compose example

```yaml
services:
  findamodel:
    image: ghcr.io/<owner>/findamodel:latest
    environment:
      - PUID=1000
      - PGID=1000
      - UMASK=022
    ports:
      - "5000:8080"
    volumes:
      - ./models:/models:ro
      - ./data:/data
    restart: unless-stopped
```

For NVIDIA GPU acceleration in Compose, add:

```yaml
    gpus: all
    environment:
      - NVIDIA_VISIBLE_DEVICES=all
  - NVIDIA_DRIVER_CAPABILITIES=compute,utility,graphics
```

If the Rendering stats panel shows `GL renderer: llvmpipe ...`, the container is still using Mesa software rendering rather than the NVIDIA driver. In that case, verify both `--gpus all` or `gpus: all` and the `graphics` driver capability are present.

FindAModel also creates `/usr/share/glvnd/egl_vendor.d/10_nvidia.json` at container startup when NVIDIA EGL libraries are mounted into the container but the GLVND vendor file is missing. This helps headless OpenGL preview rendering avoid falling back to Mesa `llvmpipe` on hosts where the NVIDIA runtime exposes the libraries but not the vendor registration.

---

## Running for development

### Using VS Code (recommended)

The repository includes a complete VS Code configuration. Open `findamodel.code-workspace` in VS Code and use the pre-configured launch profiles and tasks.

**Prerequisites:** .NET 9 SDK, Node.js, Yarn, and Docker (for the optional Seq log viewer) must be installed on your machine. Install the recommended VS Code extensions when prompted (Prettier).

**Launch profiles** (Run and Debug panel):

| Profile | Description |
|---------|-------------|
| `Full Stack: Backend + Frontend` | Starts both the backend and the frontend dev server together, and opens Seq for structured log viewing |
| `Backend: .NET Launch` | Starts only the .NET backend with the debugger attached |
| `Frontend: Vite Dev Server` | Starts only the Vite frontend dev server |
| `Seq: Dashboard` | Starts the Seq structured log viewer via Docker and opens it in the browser |

**Tasks** (Terminal → Run Task):

| Task | Description |
|------|-------------|
| `backend: build` | Builds the .NET backend |
| `backend: watch` | Builds and watches the backend for changes |
| `frontend: install` | Runs `yarn install` in the frontend directory |
| `data: clear database` | Deletes the local SQLite database |
| `data: clear cache` | Deletes the local model cache |
| `desktop-tauri: install` | Runs `yarn install` in the desktop shell directory |
| `desktop-tauri: publish` | Builds desktop bundles/installers for the current platform |

To get started:

1. Run the `frontend: install` task once to install Node dependencies.
2. Use the **Full Stack: Backend + Frontend** launch profile to start everything.
3. The frontend opens at `http://localhost:5173` and proxies API calls to the backend at `http://localhost:5000`.

---

### Manual setup

If you prefer to run without VS Code, start each process in a separate terminal.

#### Backend

```bash
cd backend
dotnet run
```

The API will start at `http://localhost:5000`.

#### Frontend

```bash
cd frontend
yarn install
yarn dev
```

The dev server will start at `http://localhost:5173` and proxy API calls to the backend.

---

## First run

1. Open the app in your browser.
2. The **Settings** page shows the configured models root path and global tuning values.
3. Optional: adjust the auto-support preview tuning in **Settings** if you want more or fewer suggested support markers for unsupported models.
4. Click **Index** on the Explore page (or via the indexer button) to scan your model files.
5. FindAModel will walk your directory tree, read any `findamodel.yaml` files it finds, and populate the model library.

---

## Adding metadata rules

Metadata is extracted automatically using `findamodel.yaml` files placed in your model directories. See the [Configuration](configuration/) and [Rules system](rules/) sections for detailed guidance.

A minimal `findamodel.yaml` to set a creator and extract model names from filenames:

```yaml
creator: "Alice"

model_name:
  source: filename
  expression: "^(.*)\\.[^./]+$"
```

When writing regex rules that should match the full path, you can omit `source` entirely because `full_path` is the default.
