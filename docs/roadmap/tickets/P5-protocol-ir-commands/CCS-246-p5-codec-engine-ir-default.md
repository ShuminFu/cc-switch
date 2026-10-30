# CCS-246: [settings] Prepare the default flip to codecEngine = ir (merge needs a maintainer release decision)

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p2-medium`, `area:settings`

> **Ticket key:** `p5-codec-engine-ir-default` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-178, CCS-006). Source tickets: [CCS-178](../P5-protocol-ir-commands/CCS-178-p5-codec-responses.md), [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
The release soak that unlocks the legacy deletion can only start once a release ships with `codecEngine = ir`, and the default is still Legacy (`src-tauri/src/proxy/codec/mod.rs:46-52` `#[default] Legacy`; `src-tauri/src/settings.rs:552` `skip_serializing_if = "CodecEngine::is_legacy"`, `:673`).

## Proposed change
- Move `#[default]` to `Ir`; set `codec_engine: CodecEngine::Ir` at settings.rs:673.
- Change the `skip_serializing_if` so an explicit `legacy` still persists (an `is_ir` helper, or always serialise).
- Update the settings serde tests.
- Open the PR as ready but do not merge until a maintainer schedules the release that starts the soak. Add a fixture change or `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [x] Unit tests: settings JSON without `codecEngine` deserialises to `Ir`; `"legacy"` round-trips to `Legacy`.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` passes, including `proxy_golden` and `proxy_e2e`.

## Dependencies
None.

## Implementation notes

**Summary.** `CodecEngine::Ir` is now the `#[default]` and `AppSettings::default()` sets `codec_engine: CodecEngine::Ir`. `CodecEngine::is_legacy` is replaced by `is_default` (same pattern as `TakeoverEngine`), so `skip_serializing_if` drops only the `ir` default and an explicit `"legacy"` is written and round-trips. Lenient parsing now falls back to the default (`ir`) for unknown strings and non-string values, matching `TakeoverEngine`.

The forwarder wire golden (`tests/fixtures/forwarder/upstream_request.expected`) runs on the default engine. It was regenerated with `CC_SWITCH_UPDATE_EXPECTED=1`. The only change is in `codex_to_anthropic_impersonating_claude_code`: `"stream": false` is no longer forwarded and `content-length` goes from 333 to 318. This is a documented IR-engine difference (CCS-178, "same rules as CCS-176/177"). `proxy_golden` legacy/IR parity and the `*_ir` snapshots did not change.

**Files**
- `src-tauri/src/proxy/codec/mod.rs`: default moved to `Ir`, `is_default`, fallbacks
- `src-tauri/src/settings.rs`: `skip_serializing_if = "CodecEngine::is_default"`, default `Ir`, doc comment
- `src-tauri/src/proxy/codec/tests.rs`: `settings_codec_engine_defaults_to_ir_and_tolerates_unknown_values` checks that missing `codecEngine` gives `Ir` and is not written out, that `"legacy"` round-trips to `Legacy` and persists, and that unknown or non-string values give `Ir`. The `codec_engine_setting_parses_leniently` test is updated.
- `src-tauri/src/proxy/codec/bridge/tests.rs`: `ir_engine_is_the_default_and_legacy_stays_a_fallback`
- `src-tauri/tests/fixtures/forwarder/upstream_request.expected`: regenerated (fixture change, see above)

**Commits:** f994f76 (code, tests, fixture) and the follow-up docs commit that records these notes.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3105 passed, 0 failed. This includes `proxy_golden` (18 passed) and `proxy_e2e` (37 passed).
- Before the fixture regeneration, the full run failed on `bridge::tests::legacy_engine_is_the_default_fallback` and `forwarder::wire_golden::upstream_request_bytes_match_golden`. Both are expected consequences of the flip and are updated above. On the old default, the new settings test fails on its first assertion, because `{}` gave `Legacy`.

**Remaining (human/maintainer):** Open the PR as ready, but do not merge it until a maintainer schedules the release that starts the soak. Apply the CCS-034 fixture label: this change includes a fixture change, the forwarder wire golden. Neither step can be done from this environment.
