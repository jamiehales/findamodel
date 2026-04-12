---
layout: default
title: Getting started (desktop)
parent: Desktop
nav_order: 1
---

# Getting started (desktop)
{: .no_toc }

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Requirements

Desktop mode requires these tools in addition to normal server/web development requirements:

- .NET 9 SDK
- Node.js 20+ and Yarn
- Rust toolchain (rustup + cargo)

Platform prerequisites:

- Windows: Microsoft Edge WebView2 runtime (usually preinstalled)
- macOS: Xcode Command Line Tools
- Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

If you just installed Rust and still see `cargo` not found, restart your terminal (or VS Code) so PATH is refreshed.

---

## Run in development mode

From repository root:

```bash
yarn --cwd desktop-tauri install
yarn --cwd desktop-tauri dev
```

This starts the Tauri desktop shell and the frontend development server.

---

## Generate installers and bundles

From repository root:

```bash
yarn --cwd desktop-tauri build
```

This build pipeline does the following:

1. Builds the frontend assets.
2. Publishes the backend sidecar binary for the current platform target.
3. Builds desktop artifacts through Tauri for your current OS.

In VS Code you can run the `desktop-tauri: publish` task for the same flow.

---

## Debug with VS Code

Use these launch profiles from Run and Debug:

- `Backend: Desktop Mode`
- `Desktop: Tauri Dev`
- `Desktop: Backend + Tauri Shell` (compound)

Use these tasks from Terminal -> Run Task:

- `desktop-tauri: install`
- `desktop-tauri: publish`

