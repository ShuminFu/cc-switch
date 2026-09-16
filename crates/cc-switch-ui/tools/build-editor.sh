#!/usr/bin/env sh
# Bundles editor-src/editor.js (CodeMirror 6) into assets/editor.js with the
# esbuild binary that dx installs. Run from crates/cc-switch-ui after
# `pnpm install` at the repository root.
set -eu
cd "$(dirname "$0")/.."
ESBUILD="${ESBUILD:-$HOME/.local/share/.dx/tools/esbuild-0.27.3/esbuild}"
if [ ! -x "$ESBUILD" ]; then ESBUILD="$(command -v esbuild)"; fi
"$ESBUILD" editor-src/editor.js --bundle --minify --format=iife --target=es2020 \
  --outfile=assets/editor.js --log-level=warning
ls -la assets/editor.js
