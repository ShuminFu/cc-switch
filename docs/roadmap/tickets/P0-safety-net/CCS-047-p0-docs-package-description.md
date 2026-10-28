# CCS-047: [docs] Update the package description to cover all supported tools

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-package-description` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-stale-description. The description lists only Claude Code, Codex and Gemini CLI, but 8 apps are supported.

## Evidence
- src-tauri/Cargo.toml:4, package.json:4; app_config.rs:28-35

## Proposed change
Use identical wording in both, for example "All-in-One Assistant for AI coding CLIs (Claude Code, Codex, Gemini CLI, OpenCode, and more)".

## Acceptance criteria / verification
- [x] `grep -rn 'Codex & Gemini CLI' package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json` finds nothing
- [x] `cargo metadata --no-deps --manifest-path src-tauri/Cargo.toml` shows the new text

## Dependencies
None.

## Implementation notes

**Summary:** `package.json` and `src-tauri/Cargo.toml` now share the identical description
"All-in-One Assistant for AI coding CLIs (Claude Code, Codex, Gemini CLI, OpenCode, and more)".
`src-tauri/tauri.conf.json` has no description field, so nothing changed there. A new vitest guard
(`tests/ci/packageDescription.test.ts`) asserts the two manifests match, that none of the three
manifest files contain the stale "Codex & Gemini CLI" wording, and that OpenCode is mentioned.

**Files:** `package.json`, `src-tauri/Cargo.toml`, `tests/ci/packageDescription.test.ts`.

**Commits:** `b21d9cc` (change + test); this ticket-file update follows as a separate commit.

**Verification:**
- `grep -rn 'Codex & Gemini CLI' package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json`: no matches (exit 1).
- `cargo metadata --no-deps --format-version 1 --manifest-path src-tauri/Cargo.toml`: description is the new text.
- `npx vitest run tests/ci/packageDescription.test.ts`: 3/3 pass; with the old `package.json` restored, all 3 fail (regression confirmed).
- `pnpm typecheck`: clean. `pnpm test:unit`: 93 files / 658 tests passed. `pnpm format:check`: clean.
- No Rust source changed, so no clippy/cargo test run was needed for this change; `cargo metadata` confirms the manifest still parses.

**Out of scope / follow-up:** the old wording is still in `src/i18n/locales/en.json` (`app.description`)
and in the Linux desktop entry `flatpak/com.ccswitch.desktop.desktop` (`Comment=`). The ticket only covers
the package manifests, so those were left as they are for a separate copy update.
