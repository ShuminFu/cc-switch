# icon-tools

Rust command line tool for maintaining the provider icons in
`src/icons/extracted`. It replaces the former `scripts/extract-icons.js` and
`scripts/filter-icons.js` Node scripts and has no external dependencies.

```bash
# from the repository root
pnpm icons:check                      # validate index.ts / metadata.ts / files
pnpm icons:extract                    # copy the built-in icon list from @lobehub/icons-static-svg
pnpm icons:filter                     # dry run of the keep-list clean-up
pnpm icons:filter -- --apply          # perform the clean-up
pnpm icons:index -- --out /tmp/i.ts   # render an index.ts from the files on disk

# or directly
cargo run --quiet --manifest-path scripts/icon-tools/Cargo.toml -- --help
```

## Commands

| Command   | What it does |
|-----------|--------------|
| `extract` | Copies the requested icons (`--icons a,b` or the built-in list) from `node_modules/@lobehub/icons-static-svg/icons`. Files that already exist locally are kept unless `--overwrite` is passed. Runs `check` afterwards when the curated files exist. |
| `filter`  | Groups `name.svg` / `name-color.svg` pairs, deletes groups that are not on the keep list and renames colour variants over the monochrome file. Files referenced by `index.ts` (by import or by derived key) or imported directly from `src/` are never touched. Dry run by default, `--apply` to execute. |
| `check`   | Validates that every import in `index.ts` exists, that keys are unique and lowercase, that inline entries are `<svg>` elements, and cross-checks `metadata.ts`. Warnings cover icons without metadata and unreferenced files. `--strict` fails on warnings, `--scaffold-metadata` prints entries to paste into `metadata.ts`. |
| `index`   | Renders a complete `index.ts` from the files in a directory: SVGs are inlined (normalised to a single line with `width`/`height` `1em` and the shared `style`), raster images and SVGs above `--inline-max-bytes` become `?url` imports. `--alias KEY=FILE` and `--ignore FILE` tune the key derivation. `--check` compares with the existing file, `--write` overwrites it. |

Exit codes: `0` success, `1` validation failed, `2` usage or I/O error.

## Differences from the Node scripts

* `index.ts` and `metadata.ts` are curated by hand today, so `extract` and
  `filter` no longer regenerate them. Use `check` to see what needs an entry
  and `index` when you want a generated file.
* `filter` is a dry run unless `--apply` is passed, and never deletes a file
  that the application references.
* `extract` does not overwrite existing local files unless `--overwrite` is
  passed; several tracked SVGs are customised versions of the upstream ones.
* `filter-icons.js` required `generate-icon-index.js`, which no longer
  existed; the `index` command fills that gap.

## Development

```bash
cd scripts/icon-tools
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
