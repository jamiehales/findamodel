# FindAModel

A self-hosted 3D model library manager. Scans directories of STL, OBJ and printer-format files, automatically extracts metadata from file and folder names using a flexible rules system, and provides a web UI for browsing, tagging, and building printing plates.

## Documentation

Full documentation is available at **[jamiehales.github.io/findamodel](https://jamiehales.github.io/findamodel/)**.

## Quick start

```bash
docker run -d \
  --name findamodel \
  -e PUID=1000 \
  -e PGID=1000 \
  -e UMASK=022 \
  -p 5000:8080 \
  -v /path/to/your/models:/models:ro \
  -v /path/to/data:/data \
  ghcr.io/jamiehales/findamodel:latest
```

For NVIDIA GPU acceleration, add:

```bash
--gpus all \
-e NVIDIA_VISIBLE_DEVICES=all
```

Or pull the image directly:

```bash
docker pull ghcr.io/jamiehales/findamodel:latest
```

See the [Getting started](docs/getting-started.md) guide for full setup instructions including Docker Compose and local development.

## Features

- **Rules-based metadata extraction** — define patterns once in `findamodel.yaml` files; creator, collection, category and more are computed automatically from your directory and file names
- **Hierarchical config** — settings cascade down the directory tree so common values only need to be set once
- **Hull-based printing plates** — pack models onto a build plate using accurate 2D hull footprints
- **WebGL model previews** — 3D previews rendered in-browser
- **Metadata filtering** — filter by creator, collection, category, material and support status

## Desktop Packaging

Desktop packaging is implemented under `desktop-tauri`.

For full desktop requirements, VS Code debug flow, and platform notes, see [Desktop getting started](docs/desktop/getting-started.md).

1. Install desktop shell dependencies:

```bash
yarn --cwd desktop-tauri install
```

2. Build and run desktop in dev mode:

```bash
yarn --cwd desktop-tauri dev
```

3. Build desktop bundles/installers:

```bash
yarn --cwd desktop-tauri build
```

The desktop shell launches the backend as a localhost sidecar with a per-session token and passes runtime API configuration to the frontend at startup.

## License

This code has used AI code generation for development, as such I don't feel it's correct to claim any ownership over the application in any way shape or form, not that I want to anyway. As such it's released under an 'unlicense' license. Essentially, do whatever you want with it! See [LICENSE](LICENSE).
