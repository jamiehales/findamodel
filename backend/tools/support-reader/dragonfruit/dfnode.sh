#!/usr/bin/env bash
# Runs a TypeScript file under plain node with DragonFruit's module resolution (tsconfig paths,
# Lingui macro stubs), using a DragonFruit checkout that is only read.
# Usage: DF_REPO=/path/to/dragonfruit dfnode.sh <script.ts> [args...]
# DF_REPO must have its node_modules installed and its generated registries in place (see README.md).
set -euo pipefail
DF="${DF_REPO:?set DF_REPO to a DragonFruit checkout (see README.md)}"
[ -d "$DF/src" ] && [ -d "$DF/node_modules" ] || { echo "DF_REPO=$DF is not a DragonFruit checkout with node_modules" >&2; exit 1; }
DF="$(cd "$DF" && pwd)"
SCRIPT="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
shift
export TSX_TSCONFIG_PATH="$DF/tsconfig.json"
# Bare imports in scripts outside the checkout resolve from the checkout's node_modules.
export NODE_PATH="$DF/node_modules"
cd "$DF"
exec node --max-old-space-size=8192 --import tsx --import ./scripts/test-unit/register.mjs "$SCRIPT" "$@"
