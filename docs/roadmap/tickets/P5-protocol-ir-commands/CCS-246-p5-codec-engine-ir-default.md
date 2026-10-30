# CCS-246: [settings] Prepare the default flip to codecEngine = ir (merge needs a maintainer release decision)

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p2-medium`, `area:settings`

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
- [ ] Unit tests: settings JSON without `codecEngine` deserialises to `Ir`; `"legacy"` round-trips to `Legacy`.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` passes, including `proxy_golden` and `proxy_e2e`.

## Dependencies
None.
