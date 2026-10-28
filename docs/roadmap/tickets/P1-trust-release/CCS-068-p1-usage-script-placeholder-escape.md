# CCS-068: [usage-script] Escape placeholder values for a JS string context

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:usage-script`

> **Ticket key:** `p1-usage-script-placeholder-escape` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-usage-script-placeholder-injection, partially real. {{apiKey}}, {{baseUrl}}, {{accessToken}} and {{userId}} are inserted raw. A value containing ", \ or a newline breaks the eval. This is a robustness bug.

## Evidence
- src-tauri/src/usage_script.rs:401-420 (build_script_with_vars); templates in src/components/UsageScriptModal.tsx:74-92,1557-1560 put the placeholders inside "..."

## Proposed change
Escape the values without adding quotes: serde_json::to_string, trim the outer quotes, and also escape ', backtick and ${. Optionally also expose `__ccs.apiKey` globals.

## Acceptance criteria / verification
- [ ] Test: api_key `ab"c\d` in `"Bearer {{apiKey}}"` evaluates to `Bearer ab"c\d`
- [ ] Ordinary keys produce output identical to before
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml usage_script`

## Dependencies
None.
