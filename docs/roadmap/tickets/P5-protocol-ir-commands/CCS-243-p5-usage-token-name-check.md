# CCS-243: [usage] Add a check script that keeps cache-token field names inside codec/ and usage/

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:usage`

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
- [x] The script passes on the current tree.
- [x] Adding a scratch `cached_tokens` line to `proxy/handlers.rs` makes it fail; reverting restores a pass.
- [x] The CI lint step runs it; `~/go/bin/actionlint .github/workflows/*.yml` exits 0.

## Dependencies
None.

## Implementation notes

**Summary.** Added `scripts/check-usage-token-names.mjs` (Node, no `rg` dependency on the runner, same shape as `check-command-bodies.mjs`). It walks every `.rs` file under `src-tauri/src`, substring-matches `cached_tokens|cache_creation_input_tokens` (same pattern as the ticket's `rg -l`), and fails on any match outside `src-tauri/src/proxy/codec/` and `src-tauri/src/proxy/usage/` unless the file is in `ALLOWLIST`. Each allowlist entry carries its removal note: the 5 legacy files (`proxy/providers/streaming.rs`, `transform.rs`, `transform_codex_anthropic.rs`, `transform_codex_chat.rs`, `transform_responses.rs`) are marked "remove with CCS-178 deletion"; `proxy/stream/taps.rs`, `services/session_usage.rs`, `proxy/providers/testkit.rs` are marked "remove when CCS-244 moves it". An allowlisted file that no longer names the fields is reported as stale and also fails, so the list has to shrink when CCS-178/CCS-244 land. The CI `ci.yml` frontend job runs it as "Check usage token names", next to the other repo-structure checks after `pnpm lint`.

**Files.**
- `scripts/check-usage-token-names.mjs`, `scripts/check-usage-token-names.d.mts` (new)
- `tests/scripts/checkUsageTokenNames.test.ts` (new, 5 tests: line detection, allowed dirs/allowlist/violation/stale logic, removal note on every entry, pass on the real tree, fail when a temp tree gains `proxy/handlers.rs` with `cached_tokens`)
- `.github/workflows/ci.yml` (new step)

**Commits.** a1b9dfe (script, tests, CI step); this ticket-notes commit follows it.

**Verification.**
- `node scripts/check-usage-token-names.mjs` on the current tree → exit 0 (8 allowlisted legacy files).
- Appended `// cached_tokens` to `src-tauri/src/proxy/handlers.rs` → exit 1, `src-tauri/src/proxy/handlers.rs:1051 cache-token field name outside proxy/codec/ and proxy/usage/`; restored the file (empty `git diff`) → exit 0.
- `~/go/bin/actionlint .github/workflows/*.yml` → exit 0; `ci.yml` parses with `yaml.safe_load`.
- `pnpm vitest run tests/scripts/checkUsageTokenNames.test.ts` → 5/5 passed; `pnpm test:unit` → 167 files / 1581 tests passed; `pnpm typecheck`, `pnpm lint`, `pnpm format:check` → clean.
- No Rust source changed, so cargo fmt/clippy/test were not needed for this ticket.
