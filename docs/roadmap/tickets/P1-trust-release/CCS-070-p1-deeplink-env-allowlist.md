# CCS-070: [deeplink] Allow-list env keys imported from Claude deeplinks and flag process-hijacking variables

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p2-medium`, `area:deeplink`

> **Ticket key:** `p1-deeplink-env-allowlist` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-deeplink-env-injection, partially real. Every env key in the base64 config is copied into the provider unfiltered and, with enabled=true, written to ~/.claude/settings.json. That includes NODE_OPTIONS, LD_PRELOAD, DYLD_* and BASH_ENV, which are then inherited by hooks, the Bash tool and MCP children. The dialog lists these keys like ordinary config, with no warning and with truncated values.

## Evidence
- src-tauri/src/deeplink/provider.rs:288-336, 344-372, 130-133; DeepLinkImportDialog.tsx:524-543, 281-290

## Proposed change
- In extract_claude_config_env, allow ANTHROPIC_*, API_TIMEOUT_MS, known CLAUDE_CODE_*/DISABLE_* and the HTTP(S)_PROXY family. Drop a deny-list (NODE_OPTIONS, NODE_PATH, LD_*, DYLD_*, PATH, BASH_ENV, ENV, PYTHONPATH, PYTHONSTARTUP).
- Show any other key in a highlighted section with its full value.
- Do not auto-switch when such keys are present.

## Acceptance criteria / verification
- [ ] Rust test: NODE_OPTIONS and LD_PRELOAD are removed; API_TIMEOUT_MS and ANTHROPIC_CUSTOM_HEADERS are kept
- [ ] Vitest: a warning section renders for non-allow-listed keys
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml deeplink` and `pnpm test:unit`

## Dependencies
None.
