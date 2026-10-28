# CCS-102: [gemini] Validate .env keys and values, strip quotes when parsing, and keep unknown lines

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:apps-gemini`

> **Ticket key:** `p1-gemini-env-writer` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-gemini-env-injection.
- serialize_env_file writes raw KEY=value, so a deeplink apiKey containing %0A injects lines.
- The parser keeps surrounding quotes.
- Every write drops comments and lines the user added by hand.

## Evidence
- src-tauri/src/gemini_config.rs:26-50, 126-140, 156-190, 204-216; deeplink/parser.rs:99; deeplink/provider.rs:432-445; live.rs:1638/1643

## Proposed change
- Reject values containing \r, \n or \0, and keys that do not match [A-Za-z_][A-Za-z0-9_]*.
- Strip one pair of matching quotes and handle `export `.
- Merge on write, keeping comment lines and unknown keys.
- Reject control characters in the deeplink apiKey and model.

## Acceptance criteria / verification
- [ ] Tests: a value "k\nEVIL=1" gives Err; a quoted base URL parses without quotes; a round-trip keeps comments
- [ ] Deeplink test: an apiKey with %0A for gemini is rejected
- [ ] P0 gemini writer snapshot updated intentionally
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml gemini_config deeplink`

## Dependencies
p0-config-writer-snapshots
