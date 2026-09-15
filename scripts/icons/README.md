# cc-switch-icons

Rust command-line tool that maintains the provider icons bundled with CC Switch
(`src/icons/extracted`). It replaces the former `extract-icons.js` and
`filter-icons.js` Node scripts and has no third-party dependencies.

The icon directory is hand-curated: `index.ts` inlines every SVG and imports
raster or oversized icons by URL, while `metadata.ts` holds display names,
categories, keywords and colours. The tool never rewrites those files wholesale.
It only appends entries that are missing, so hand-tuned icons survive every run.

## Commands

Run through pnpm from the repository root:

```bash
pnpm icons:check              # verify files, index.ts and metadata.ts agree
pnpm icons:extract            # pull the default icon set from @lobehub/icons-static-svg
pnpm icons:extract openai     # pull specific icons
pnpm icons:filter             # dry run: list unused SVGs and colour variants to rename
pnpm icons:filter --apply     # actually delete / rename
```

Or call cargo directly:

```bash
cargo run --manifest-path scripts/icons/Cargo.toml -- help
```

### extract

Copies `<name>.svg` from `node_modules/@lobehub/icons-static-svg/icons` into the
bundle. An icon that is missing upstream but present locally is kept; one that
is missing in both places is reported. Every available icon that `index.ts` does
not know yet is registered: small SVGs are normalised (single line, `1em`
sizing, shared `style`, `<title>`) and inlined, SVGs with embedded images or
over 32 KiB are imported with Vite's `?url` suffix. A metadata entry is added
when `metadata.ts` has none, using built-in values for well-known providers and
a stub otherwise. `--readme` also writes a summary `README.md` next to the icons.

### filter

Computes which SVG files are neither in the built-in "famous icons" list nor
referenced by `index.ts`, and which `-color` variants should replace their
monochrome twin. Nothing is changed unless `--apply` is given. `--keep a,b`
protects additional base names.

### check

Reports errors (duplicate or upper-case keys, imports of missing files, SVG
imports without `?url`, URL entries without an import) and warnings (icons
without metadata, metadata without an icon, files nothing references). Exits
non-zero on errors; `--strict` also fails on warnings. CI runs this command.

## Development

```bash
cd scripts/icons
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
