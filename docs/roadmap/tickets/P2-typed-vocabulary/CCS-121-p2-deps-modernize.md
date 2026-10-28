# CCS-121: [deps] Replace serde_yaml, consolidate the JSON5 crates and drop once_cell

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p3-low`, `area:deps`

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
