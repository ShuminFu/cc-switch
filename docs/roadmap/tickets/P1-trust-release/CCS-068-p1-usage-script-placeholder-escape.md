# CCS-068: [usage-script] Escape placeholder values for a JS string context

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:usage-script`

> **Ticket key:** `p1-usage-script-placeholder-escape` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-usage-script-placeholder-injection, partially real. {{apiKey}}, {{baseUrl}}, {{accessToken}} and {{userId}} are inserted raw. A value containing ", \ or a newline breaks the eval. This is a robustness bug.

## Evidence
- src-tauri/src/usage_script.rs:401-420 (build_script_with_vars); templates in src/components/UsageScriptModal.tsx:74-92,1557-1560 put the placeholders inside "..."

## Proposed change
Escape the values without adding quotes: serde_json::to_string, trim the outer quotes, and also escape ', backtick and ${. Optionally also expose `__ccs.apiKey` globals.

## Acceptance criteria / verification
- [x] Test: api_key `ab"c\d` in `"Bearer {{apiKey}}"` evaluates to `Bearer ab"c\d`
- [x] Ordinary keys produce output identical to before
- [x] `cargo test --manifest-path src-tauri/Cargo.toml usage_script`

## Dependencies
None.

## Implementation notes
- **Summary:** `build_script_with_vars` now passes every placeholder value through a new `escape_js_string_content` helper: `serde_json::to_string` with the outer quotes trimmed (handles `"`, `\`, newlines/control chars), then `'`, backtick and `${` are backslash-escaped. Values therefore evaluate literally inside `"..."`, `'...'` and `` `...` `` literals. Ordinary values (alphanumerics, `-_.:/`) are byte-identical to before. The optional `__ccs.*` globals were not added (left out of scope).
- **Files:** `src-tauri/src/usage_script.rs` (helper + 3 tests: `usage_script_placeholder_with_quote_and_backslash_evaluates_literally`, `usage_script_placeholder_values_survive_every_string_context`, `usage_script_ordinary_placeholder_values_are_unchanged`).
- **Commits:** ae6286a (code + tests); ticket notes in the follow-up docs commit.
- **Verification:**
  - `cargo fmt --manifest-path src-tauri/Cargo.toml` — clean
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — clean
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib usage_script` — 15 passed, 0 failed
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib` — 2173 passed, 0 failed, 3 ignored
  - Before the fix, the raw splice produced `"Bearer ab"c\d"`, a JS syntax error, so the first test fails without the change.
