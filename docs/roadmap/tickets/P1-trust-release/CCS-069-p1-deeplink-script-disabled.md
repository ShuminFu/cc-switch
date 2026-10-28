# CCS-069: [deeplink] Store deeplink-delivered usage scripts disabled until the user reviews and enables them

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:deeplink`

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
- [ ] deeplink/tests.rs: a request with a script and usage_enabled None, and one with Some(true), both give enabled == false
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml deeplink`

## Dependencies
p1-usage-script-sandbox
