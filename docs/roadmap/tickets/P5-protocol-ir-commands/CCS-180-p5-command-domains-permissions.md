# CCS-180: [ipc] Register commands per domain with collect_commands! and scope them with AppManifest permissions

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P5`, `p2-medium`, `area:ipc`

> **Ticket key:** `p5-command-domains-permissions` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-misc-split-terminal`, `p2-bindings-generate`, `p1-ipc-path-validation`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-ipc-commands-open, partially real. All ~289 commands are callable from the webview, including file-writing primitives. The strict CSP makes exploitation hard, so this is defence in depth. The critique sequences it after the domain split.

## Evidence
- src-tauri/build.rs:2 plain tauri_build::build(); capabilities/default.json; lib.rs generate_handler

## Proposed change
- Add commands/<domain>/ folders using collect_commands!.
- build.rs uses `try_build(Attributes::new().app_manifest(AppManifest::new().commands(&[...])))`.
- Add src-tauri/permissions/<domain>.toml, and put privileged commands (env restore/delete, terminals, file writers) in a separate set that is not in default.json.

## Acceptance criteria / verification
- [ ] `cargo build` produces gen/schemas listing the app permissions
- [ ] From devtools, invoking a command that was not granted fails with "not allowed"
- [ ] tauri-driver smoke is green

## Dependencies
p5-misc-split-terminal, p2-bindings-generate, p1-ipc-path-validation
