# Dioxus UI: developer setup

The Rust frontend lives in `crates/cc-switch-ui` (Dioxus 0.7, web renderer,
compiled to `wasm32-unknown-unknown`) and talks to the unchanged Tauri backend
in `src-tauri` through `window.__TAURI__`. Shared wire types and command names
are in `crates/cc-switch-contract`. See `dioxus-migration-plan.md` for the
overall plan and phase status.

## Layout

```
crates/
  Cargo.toml              # workspace: cc-switch-contract, cc-switch-ui (target dir: crates/target)
  cc-switch-contract/     # serde wire types, command/event name constants, IpcError
  cc-switch-ui/           # Dioxus app: src/main.rs, src/ipc.rs (invoke/listen bridge), src/api/*
    assets/base.css       # copy of src/index.css (design tokens, glass helpers, drag regions)
    tailwind.css          # Tailwind input; dx writes assets/tailwind.css (gitignored)
    tailwind.config.js    # same theme as the React app, content globs on *.rs
src-tauri/tauri.dioxus.conf.json   # overlay config: devUrl :3001, frontendDist = dx output, withGlobalTauri, wasm CSP
src-tauri/tests/ipc_contract.rs    # every contract command is registered; serde shapes agree
```

Deviation from the original plan: `src-tauri` is *not* a workspace member.
Its release profile, `Cargo.lock` and the `src-tauri/target` paths used by
`release.yml` stay untouched; it depends on the contract crate by path.

## Prerequisites

```bash
rustup target add wasm32-unknown-unknown
# Dioxus CLI 0.7.10 (prebuilt; or `cargo install dioxus-cli --version 0.7.10`)
curl -sSL https://github.com/DioxusLabs/dioxus/releases/download/v0.7.10/dx-x86_64-unknown-linux-gnu.tar.gz | tar xz && install -m 755 dx ~/.cargo/bin/dx
```

`dx` downloads its helper tools on first use (esbuild, wasm-bindgen,
binaryen, the standalone Tailwind binary) into `~/.local/share/.dx/tools/`
(override with `DX_HOME`). Behind a TLS-intercepting proxy dx cannot use the
system CA bundle; place the binaries there by hand:

```
~/.local/share/.dx/tools/esbuild-0.27.3/esbuild                  # npm @esbuild/linux-x64 0.27.3
~/.local/share/.dx/tools/wasm-bindgen-0.2.128/wasm-bindgen       # must match the wasm-bindgen crate version in crates/Cargo.lock
~/.local/share/.dx/tools/binaryen-129/bin/wasm-opt               # binaryen version_129
~/.local/share/.dx/tools/tailwindcss-v3.4.15/tailwindcss         # tailwindcss standalone v3.4.15
```

## Generated sources

| Generator | Output | Source of truth (until Phase 6) |
|---|---|---|
| `python3 scripts/gen-contract-commands.py` | `crates/cc-switch-contract/src/commands.rs` | `src/lib/api/*.ts` invoke calls |
| `node crates/cc-switch-ui/tools/gen_api.mjs` | `crates/cc-switch-ui/src/api/*.rs` (except `app.rs`) | `src/lib/api/*.ts` method signatures |
| `pnpm presets:dump` | `crates/cc-switch-presets/data/*.json` | `src/config/*Presets.ts` |
| `pnpm icons:emit-rust` | `crates/cc-switch-ui/src/icons/generated.rs`, `assets/icons/` | `src/icons/extracted/{index,metadata}.ts` |

Locale files are embedded directly from `src/i18n/locales/*.json`.

## Commands

```bash
pnpm dev:dioxus               # Tauri window with the Dioxus UI (dx serve on :3001)
pnpm build:dioxus             # release bundle with the Dioxus UI
pnpm dev                      # the React UI, unchanged, still the default

cd crates
cargo test -p cc-switch-contract
cargo clippy -p cc-switch-ui --target wasm32-unknown-unknown -- -D warnings
cargo fmt --all -- --check
cd cc-switch-ui && dx build --platform web --release   # -> crates/target/dx/cc-switch-ui/release/web/public

cd src-tauri && cargo test --test ipc_contract            # contract vs generate_handler! and serde shapes
```

`dx serve` outside Tauri (plain browser) shows an explicit "window.__TAURI__
is missing" error instead of hanging; the IPC bridge only works inside the
Tauri webview or with a mocked `window.__TAURI__` (see the headless smoke
test in `crates/cc-switch-ui/e2e/`).
