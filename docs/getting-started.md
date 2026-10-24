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
- Or: a Rust toolchain (rustup + cargo) and Node.js 24+ with Yarn for local development.

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

Open the app and complete the setup wizard, entering `/models` as the model root path. Alternatively set `FINDAMODEL_MODELS_PATH=/models` to skip the wizard.

If you want NVIDIA GPU acceleration for preview rendering, add:

```bash
--gpus all \
-e NVIDIA_VISIBLE_DEVICES=all \
-e NVIDIA_DRIVER_CAPABILITIES=compute,utility,graphics
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FINDAMODEL_MODELS_PATH` | _(unset)_ | Model root inside the container (e.g. `/models`). When unset, the setup wizard asks for it |
| `FINDAMODEL_DATA_PATH` | `/data` | Path for the database and preview cache |
| `FINDAMODEL_LISTEN` | `0.0.0.0:8080` | Address and port the server binds to |
| `FINDAMODEL_INDEX_SCHEDULE` | `0 3 * * *` | Cron schedule (UTC) for the full re-index |
| `FINDAMODEL_INDEX_ON_STARTUP` | `false` | Sync directory configs when the server starts |
| `FINDAMODEL_FILE_SCAN_THREADS` | `10` | Number of model files processed in parallel while indexing |
| `FINDAMODEL_PREVIEW_USE_GPU` | `true` | Render previews on the GPU when one is available |
| `FINDAMODEL_LOG` | `warn,findamodel=info` | Log filter (e.g. `findamodel=debug`) |
| `PUID` | `1000` | Run the app process as this user id (LinuxServer style) |
| `PGID` | `1000` | Run the app process as this group id (LinuxServer style) |
| `UMASK` | `022` | Umask applied before starting the app process |
| `UMASK_SET` | _(deprecated)_ | Legacy alias for `UMASK` for LinuxServer compatibility |
| `NVIDIA_VISIBLE_DEVICES` | `all` | NVIDIA device visibility for preview rendering (`none` to force CPU rendering) |
| `NVIDIA_DRIVER_CAPABILITIES` | `compute,utility,graphics` | Required NVIDIA driver capabilities for GPU preview rendering |

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

Previews are rendered on the GPU through Vulkan (or OpenGL as a fallback). Without a usable GPU, FindAModel uses its built-in multithreaded CPU renderer. The **Rendering** section of **Settings > Stats** shows which renderer is in use; if it shows `CPU rasteriser`, verify both `--gpus all` or `gpus: all` and the `graphics` driver capability are present.

At startup the container also registers the NVIDIA Vulkan ICD and GLVND EGL vendor files when the driver libraries are mounted but those registrations are missing.

---

## Running for development

### Using VS Code (recommended)

The repository includes a complete VS Code configuration. Open `findamodel.code-workspace` in VS Code and use the pre-configured launch profiles and tasks.

**Prerequisites:** a Rust toolchain (rustup + cargo), Node.js and Yarn must be installed on your machine. Install the recommended VS Code extensions when prompted (Prettier, rust-analyzer, CodeLLDB).

**Launch profiles** (Run and Debug panel):

| Profile | Description |
|---------|-------------|
| `Full Stack: Backend + Frontend` | Starts both the backend and the frontend dev server together |
| `Backend: Rust Launch` | Starts only the backend with the debugger attached |
| `Frontend: Vite Dev Server` | Starts only the Vite frontend dev server |

**Tasks** (Terminal → Run Task):

| Task | Description |
|------|-------------|
| `backend: build` | Builds the backend |
| `backend: test` | Runs the backend test suite |
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
FINDAMODEL_DATA_PATH=debug/data FINDAMODEL_MODELS_PATH=/path/to/your/models cargo run -p findamodel
```

The API will start at `http://localhost:5000`. Run the backend tests with `cargo test -p findamodel`.

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
2. The **Settings** page shows the configured models root path.
3. Click **Index** on the Explore page (or via the indexer button) to scan your model files.
4. FindAModel will walk your directory tree, read any `findamodel.yaml` files it finds, and populate the model library.

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
