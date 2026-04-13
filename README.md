# FindAModel

A self-hosted 3D model library manager. Scans directories of STL, OBJ and printer-format files, automatically extracts metadata from file and folder names using a flexible rules system, and provides a web UI for browsing, tagging, and building printing plates.

## Features

- **Rules-based metadata extraction** - define patterns once in `findamodel.yaml` files that stay within your file tree. creator, collection, category and more are computed automatically from your directory and file names
- **Hierarchical config** - settings cascade down the directory tree so common values only need to be set once, but can be overridden at any layer
- **Printing plate management** - add models to printing lists, and support packing models onto a build plate, including support for raft overlapping
- **WebGL model previews** - 3D previews rendered in-browser, including dynamic detection and optional removal of supports so you can view the model more easily
- **Quick search** - stores the metadata in a database to search across tens of thousands of models instantly

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

## Desktop Support

There is desktop support, but it's not very well tested, see [Desktop getting started](docs/desktop/getting-started.md).

## License

This project has used AI code generation for development, as such I don't feel it's correct to claim any ownership over the application in any way shape or form, not that I want to anyway. As such it's released under an 'unlicense' license. Essentially, do whatever you want with it! See [LICENSE](LICENSE).
