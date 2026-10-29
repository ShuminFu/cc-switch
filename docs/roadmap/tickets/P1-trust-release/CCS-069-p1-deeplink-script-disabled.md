# CCS-069: [deeplink] Store deeplink-delivered usage scripts disabled until the user reviews and enables them

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:deeplink`

> **Ticket key:** `p1-deeplink-script-disabled` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p1-usage-script-sandbox`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-deeplink-autoenable-script, partially real. A deeplink stores its usage script enabled by default. The confirm dialog shows only an "Enabled" badge, not the code or the target host. Once enabled, the script can send the provider's credentials to any URL.

## Evidence
- src-tauri/src/deeplink/provider.rs:246-255 `usage_enabled.unwrap_or(!code.is_empty())`; deeplink/mod.rs:117; DeepLinkImportDialog.tsx:626-650; services/provider/usage.rs:231

## Proposed change
- Use `usage_enabled.unwrap_or(false)` and ignore a link-supplied `usageEnabled=true`.
- Enabling requires the UI toggle, which shows the decoded code and the target host.

## Acceptance criteria / verification
- [x] deeplink/tests.rs: a request with a script and usage_enabled None, and one with Some(true), both give enabled == false
- [x] `cargo test --manifest-path src-tauri/Cargo.toml deeplink`

## Dependencies
p1-usage-script-sandbox

## Implementation notes
- **Summary:** `build_provider_meta` now always stores a deeplink-delivered usage script with `enabled = false`, ignoring both the implicit "has code" default and a link-supplied `usageEnabled=true`. The parser normalises `usage_enabled` to `Some(false)` whenever the link carries `usageScript` or `usageEnabled`, so the existing confirm dialog (`DeepLinkImportDialog.tsx`, badge driven by `request.usageEnabled !== false`) now shows "Disabled" without a frontend change. Enabling is done afterwards via the provider's usage script settings (UsageScriptModal), where the decoded code, including any target URL, is shown in the editor.
- **Files:** `src-tauri/src/deeplink/provider.rs`, `src-tauri/src/deeplink/parser.rs`, `src-tauri/src/deeplink/mod.rs` (doc comment), `src-tauri/src/deeplink/tests.rs`.
- **Tests:** new `test_deeplink_usage_script_is_stored_disabled` (script with `usage_enabled` None / Some(true) / Some(false) all give `enabled == false`; fails before the change for None and Some(true)), new `test_parse_deeplink_ignores_usage_enabled_true`; `test_deeplink_usage_script_does_not_copy_provider_credentials` updated to expect disabled for `Some(true)`.
- **Commits:** `03e2c8b` (code + tests); this ticket note in a follow-up commit.
- **Verification:**
  - `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: no warnings
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib deeplink`: 42 passed, 0 failed
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2175 passed, 0 failed, 3 ignored
  - `cargo test --manifest-path src-tauri/Cargo.toml --test deeplink_import`: 2 passed
- **Follow-up (optional, frontend lane):** the confirm dialog still shows only a badge; showing the decoded script and its target host inside the dialog itself was not done here because `DeepLinkImportDialog.tsx` is being edited concurrently in the frontend lane.
