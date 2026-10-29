# CCS-070: [deeplink] Allow-list env keys imported from Claude deeplinks and flag process-hijacking variables

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p2-medium`, `area:deeplink`

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
- [x] Rust test: NODE_OPTIONS and LD_PRELOAD are removed; API_TIMEOUT_MS and ANTHROPIC_CUSTOM_HEADERS are kept
- [x] Vitest: a warning section renders for non-allow-listed keys
- [x] `cargo test --manifest-path src-tauri/Cargo.toml deeplink` and `pnpm test:unit`

## Dependencies
None.

## Implementation notes

**Summary.** `extract_claude_config_env` (src-tauri/src/deeplink/provider.rs) now classifies every env key from the inline Claude config via `classify_claude_env_key` (case-insensitive):
- *Allowed*: `ANTHROPIC_*`, `CLAUDE_CODE_*`, `DISABLE_*`, `API_TIMEOUT_MS`, `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`/`ALL_PROXY` (any case).
- *Denied* (dropped, with a `log::warn!`): `NODE_OPTIONS`, `NODE_PATH`, `PATH`, `BASH_ENV`, `ENV`, `PYTHONPATH`, `PYTHONSTARTUP`, `LD_*`, `DYLD_*`.
- *Unlisted* (anything else): still imported, but `import_provider_from_deeplink` skips the `enabled=true` auto-switch for Claude / Claude Desktop when any such key is present.

The import dialog mirrors the lists in `src/lib/deeplink/envKeys.ts` (`classifyClaudeEnvKey`). Allowed keys render as before. Unlisted keys get a highlighted red section (`data-testid="deeplink-unlisted-env"`) showing their full values, unmasked and untruncated, plus a "will not auto-switch" note when the link sets `enabled`. Denied keys are listed by name in a "removed for safety" box (`deeplink-denied-env`). New i18n keys `deeplink.unlistedEnvTitle`, `unlistedEnvDescription`, `unlistedEnvNoAutoSwitch` and `deniedEnvTitle` were added to en/zh/zh-TW/ja.

**Files.**
- src-tauri/src/deeplink/provider.rs
- src-tauri/src/deeplink/tests.rs (`test_build_claude_provider_drops_process_hijacking_env_keys`, `test_classify_claude_env_key`)
- src-tauri/tests/deeplink_import.rs (`deeplink_import_with_unlisted_env_keys_does_not_auto_switch`)
- src/lib/deeplink/envKeys.ts (new)
- src/components/DeepLinkImportDialog.tsx
- src/i18n/locales/{en,zh,zh-TW,ja}.json
- tests/components/DeepLinkImportDialog.test.tsx (two new cases)

**Commits.** 1b51fbe (backend), 8e355c5 (dialog + i18n), plus this ticket-notes commit.

**Verification.**
- `cargo fmt`: clean. `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml deeplink`: lib 44 passed, `deeplink_import` integration 3 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2177 passed, 0 failed, 3 ignored.
- Frontend, run on a checkout-index export of the committed tree, because the shared working tree holds another lane's uncommitted i18n edits: `tsc --noEmit` clean; `vitest run` 117 files / 1227 tests passed; prettier check clean on the touched files.
- Regression: the new vitest case "highlights non-allow-listed env keys with their full values" fails against the previous dialog and passes now. The Rust drop test asserts that `NODE_OPTIONS`/`LD_PRELOAD` are absent, and the previous code kept them.
