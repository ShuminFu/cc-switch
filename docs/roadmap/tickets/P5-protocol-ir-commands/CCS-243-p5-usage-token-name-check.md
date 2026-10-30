# CCS-243: [usage] Add a check script that keeps cache-token field names inside codec/ and usage/

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:usage`

> **Ticket key:** `p5-usage-token-name-check` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-178, CCS-006). Source tickets: [CCS-178](../P5-protocol-ir-commands/CCS-178-p5-codec-responses.md), [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
The CCS-178 / P5 criterion "token names only in codec and usage modules" is checked by hand with grep. `cached_tokens` / `cache_creation_input_tokens` appear outside `proxy/codec/` and `proxy/usage/` in the 5 legacy transform/streaming files, `proxy/stream/taps.rs`, `services/session_usage.rs` and `proxy/providers/testkit.rs`, and nothing stops new uses.

## Proposed change
- Add `scripts/check-usage-token-names.sh` (or .mjs) and wire it into the existing lint step: `rg -l 'cached_tokens|cache_creation_input_tokens' src-tauri/src`, failing on any match outside `proxy/codec/` and `proxy/usage/` except an explicit allowlist:
  - the 5 legacy files (`streaming.rs`, `transform.rs`, `transform_codex_anthropic.rs`, `transform_codex_chat.rs`, `transform_responses.rs`), each marked "remove with CCS-178 deletion";
  - `proxy/stream/taps.rs`, `services/session_usage.rs`, `proxy/providers/testkit.rs` (until CCS-244 moves them).

## Acceptance criteria / verification
- [ ] The script passes on the current tree.
- [ ] Adding a scratch `cached_tokens` line to `proxy/handlers.rs` makes it fail; reverting restores a pass.
- [ ] The CI lint step runs it; `~/go/bin/actionlint .github/workflows/*.yml` exits 0.

## Dependencies
None.
