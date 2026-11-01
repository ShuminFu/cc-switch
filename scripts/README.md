# scripts/

Repository tooling. Everything here runs with the Node version in `.node-version` and has no
dependencies beyond `node_modules` (Node scripts) or a POSIX shell.

| Script | Run with | What it does |
| --- | --- | --- |
| `bump-version.mjs` | `pnpm release:bump X.Y.Z`, `node scripts/bump-version.mjs --check [--tag vX.Y.Z]` | Writes / checks the app version in `package.json`, `Cargo.toml`, `Cargo.lock` and `tauri.conf.json` (CI runs `--check`) |
| `release-notes.mjs` | `pnpm release:notes --tag vX.Y.Z --body out.md --updater-notes notes.txt` | Renders the GitHub release body and the in-app updater notes from `docs/release-notes/vX.Y.Z-*.md`; `--check` verifies all three languages exist. Used by `release.yml` |
| `check-i18n.mjs` | `pnpm i18n:check` | Locale files are valid, complete and consistent |
| `check-repo.mjs` | `pnpm check:repo` | Labeler globs match files, docs only name release assets that exist, the four READMEs share one structure, product descriptions list every tool, no new image over 500 KB |
| `bundle-report.mjs` | `pnpm bundle:report` (or after `pnpm build:renderer`) | JS/CSS sizes of `dist/` with gzip, fails over the budget; CI writes it to the job summary |
| `extract-icons.mjs` | `pnpm icons:extract [--color] <name>...`, `--list` | Copies provider icons from `@lobehub/icons-static-svg` into `src/icons/extracted/` and prints the `index.ts` / `metadata.ts` entries to paste (those two files are maintained by hand) |
| `gen-bindings.mjs` | `pnpm gen:bindings` | Regenerates `src/lib/bindings.ts` from the Rust command signatures |
| `install-linux-deps.sh` | `scripts/install-linux-deps.sh [--bundle]` | Installs the Debian/Ubuntu packages needed to build (and with `--bundle`, to package) the app; shared by CI, the release workflow and the devcontainer |
| `ci/notarize-macos.sh` | release workflow | Notarizes and staples a macOS `.app` or `.dmg` (API key preferred, Apple ID fallback); retries only submission errors |
| `ci/smoke-deb.sh` | release workflow | Installs a built `.deb` on the runner and checks binary, linked libraries and desktop entry |

Other package scripts that do not live here: `pnpm deadcode` (unused files and exports, via
`knip`) and `pnpm bundle:visualize` (treemap of the renderer bundle in
`reports/bundle-treemap.html`) download their tool on first use with `pnpm dlx`.

Tests for these scripts are in `tests/scripts/`.
