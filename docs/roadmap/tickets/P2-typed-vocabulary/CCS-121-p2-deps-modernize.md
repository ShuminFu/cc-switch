# CCS-121: [deps] Replace serde_yaml, consolidate the JSON5 crates and drop once_cell

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p3-low`, `area:deps`

> **Ticket key:** `p2-deps-modernize` · **Phase:** P2 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-deprecated-deps, partially real.
- serde_yaml 0.9.34+deprecated is archived, and hermes_config.rs and mcp/hermes.rs use it heavily.
- json5 and json-five overlap; json-five is used for round-trip editing.
- once_cell can be replaced by std LazyLock and OnceLock.

## Evidence
- src-tauri/Cargo.toml:70, 74, 82-83; openclaw_config.rs:206,220,342,364,496; provider_defaults.rs, tray.rs, commands/misc.rs, proxy/http_client.rs

## Proposed change
- Move to serde_norway, not serde_yml.
- Use json_five::from_str after an edge-case parity test.
- Switch to std::sync::LazyLock and OnceLock.

## Acceptance criteria / verification
- [ ] Hermes and OpenClaw writer snapshots are unchanged
- [ ] JSON5 fixtures (comments, trailing commas, unquoted keys, single quotes) parse identically
- [ ] `cargo tree -i serde_yaml` and `cargo tree -i once_cell` show them gone as direct dependencies
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p0-config-writer-snapshots

## Implementation notes

**Summary**
- `serde_yaml` 0.9.34+deprecated replaced by `serde_norway` 0.9 (API-compatible fork); all `serde_yaml::` uses in `hermes_config.rs`, `mcp/hermes.rs` and `services/skill.rs` renamed to `serde_norway::`.
- `json5` removed; the five `json5::from_str` call sites (`openclaw_config.rs` read / health scan / post-write scans, `opencode_config.rs` read) now use `json_five::from_str`, so `json-five` is the only JSON5 crate (it was already used for round-trip editing).
- `once_cell` removed: `Lazy` -> `std::sync::LazyLock` (`provider_defaults.rs`, `tray.rs`, `commands/misc.rs`), `OnceCell` -> `std::sync::OnceLock` (`proxy/http_client.rs`). MSRV 1.88 covers both.
- New test `openclaw_config::tests::json5_fixtures_parse_like_previous_parser` pins the JSON5 fixtures (line/block/trailing comments, trailing commas, unquoted keys incl. `$`/`_`, single quotes and escaped quotes, `\u`/`\x`/`\0`/`\v` escapes, hex, signed/leading-dot/trailing-dot numbers, Infinity/NaN -> null, duplicate keys last-wins, BOM, nested OpenClaw-shaped config, and error cases). Expected values were captured by running both `json5` 0.4.1 and `json-five` 0.3.1 side by side in a scratch crate; all pinned cases were identical. `read_openclaw_config_accepts_json5_syntax` exercises the real read path.
- Parity divergences found (not pinned, rare in real configs): line continuations (`\` + newline) keep the newline in json-five but are removed by json5; `\a` is BEL in json-five but `a` in json5; unknown escapes such as `\q` error in json-five but give `q` in json5; float overflow (`1e400`) is `null` in json-five but an error in json5.
- Writer snapshots: Hermes snapshots unchanged. OpenClaw `lf`/`crlf`/`bom` snapshots: written file contents unchanged; only the parser error message of the already-pinned json-five block-comment known bug changed (now json-five's `Unexpected end of input while processing block comment` instead of json5's pest diagnostic).

**Files**: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/{hermes_config.rs,mcp/hermes.rs,services/skill.rs,openclaw_config.rs,opencode_config.rs,provider_defaults.rs,tray.rs,commands/misc.rs,proxy/http_client.rs}`, `src-tauri/tests/fixtures/live/openclaw/{lf,crlf,bom}.snap`.

**Commits**: `437cff7` (code), plus the follow-up commit recording these notes.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2384 passed, 0 failed (includes `config_writers` 16/16).
- `cargo tree -i serde_yaml` / `cargo tree -i json5`: "did not match any packages"; `cargo tree -i once_cell -e normal --depth 1`: `cc-switch` no longer listed (still pulled transitively by tauri/gtk crates).
