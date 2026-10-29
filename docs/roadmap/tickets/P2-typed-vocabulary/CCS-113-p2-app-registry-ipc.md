# CCS-113: [apps] Expose get_app_registry, add the exhaustiveness test, and fix the prompt and skills-dir edge cases

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:apps`

> **Ticket key:** `p2-app-registry-ipc` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p2-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The frontend duplicates capability knowledge. prompt_files.rs has an unreachable!() for some apps, and Claude Desktop reports a bogus skills dir.

## Evidence
- prompt_files.rs:31/38 unreachable!; skill.rs:544

## Proposed change
- Add a get_app_registry command that returns the descriptors.
- Add an exhaustiveness test: every variant has a descriptor, an i18n label in all locales, an icon and a presets file.
- Return a typed Unsupported error instead of panicking, and return None for Claude Desktop skills.

## Acceptance criteria / verification
- [x] The exhaustiveness test fails when a dummy variant lacks a label
- [x] Test: the prompt path for each app is Ok or Unsupported, with no panic
- [x] `cargo test --manifest-path src-tauri/Cargo.toml apps::`

## Dependencies
p2-app-registry

## Implementation notes

**Summary.**
- New `get_app_registry` Tauri command (`src-tauri/src/commands/apps.rs`, registered in `lib.rs`).
  It returns `Vec<AppDescriptorDto>` built from `apps::APPS` by `registry_dto()`. Each entry has
  camelCase `id`, `aliases`, `labelKey`, `displayName`, `syncMode`
  (`switch|additive`), `caps` (`mcp/skills/prompts/proxyTakeover/tray` booleans), `proxyRoute`,
  `inputTokenSemantics` (`fresh|cacheInclusive`) and `defaultVisible`. The frontend wrapper is
  `appsApi.getRegistry()` in `src/lib/api/apps.ts` (types `BackendAppDescriptor`/`BackendAppCaps`).
  The static `src/entities/app/registry.ts` was not switched to the backend data, because the
  ticket only asks for the command and its callers use it synchronously. That switch is one
  `useAppRegistry()` change for a later ticket.
- New typed error `AppError::Unsupported { app, feature }`, built with `AppError::unsupported(&app, "Prompts")`.
  `prompt_file_path` returns it for any app without `AppCaps::PROMPTS` (Claude Desktop). The two
  `unreachable!()` arms are gone: one exhaustive match now returns `(base_dir, filename)`.
- `SkillService::get_app_skills_dir` now returns `Result<Option<PathBuf>>`, with `None` for
  Claude Desktop. Before, it returned a bogus `~/.claude-desktop/skills`, and scan/import/uninstall
  backup treated that path as a source. Callers skip `None`.
- Exhaustiveness test `apps::registry::tests::every_app_has_descriptor_labels_icon_and_presets`
  checks that every `AppType` variant (exhaustive match) has a descriptor. Each descriptor must
  also have its `label_key` in en/zh/zh-TW/ja, an `APP_ICON_MAP` entry in
  `src/config/appConfig.tsx`, an `id: "<slug>"` entry in `src/entities/app/registry.ts`, and a
  `src/features/providers/forms/<slug>/presets.ts`. The check is the pure function
  `exhaustiveness_gaps`, so `exhaustiveness_check_flags_dummy_app_without_label` can confirm that
  a dummy descriptor with no label is flagged in every locale (along with its missing icon and
  presets). It also confirms that removing a single zh label is reported.

**Files.** `src-tauri/src/apps/registry.rs`, `src-tauri/src/commands/apps.rs` (new),
`src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/error.rs`,
`src-tauri/src/prompt_files.rs`, `src-tauri/src/services/skill.rs`, `src/lib/api/apps.ts` (new),
`src/lib/api/index.ts`, `tests/lib/appsApi.test.ts` (new).

**Commits.** 7f7e3ba (code), plus this ticket-notes commit.

**Tests added.** `prompt_files::tests::prompt_path_is_ok_or_unsupported_for_every_app`
(catch_unwind over all apps; Ok only with PROMPTS cap, otherwise `Unsupported`) and
`claude_desktop_prompt_path_is_unsupported`. Both fail on the old code, which returned
`AppError::Localized`. Also added `services::skill::tests::claude_desktop_has_no_skills_dir`,
`apps::registry::tests::registry_dto_mirrors_descriptors` (caps and JSON shape), and a vitest test
showing `appsApi.getRegistry` invokes `get_app_registry`.

**Verification.**
- `cargo fmt --check` -> clean; `cargo clippy --all-targets -- -D warnings` -> clean
- `cargo test --lib -- apps:: prompt_files claude_desktop_has_no_skills_dir get_app_skills_dir` -> 19 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` -> 2373 passed, 0 failed (lib 2215 passed / 3 ignored)
- `pnpm typecheck` -> clean; `pnpm test:unit` -> 147 files / 1441 tests passed; `pnpm format:check` -> clean
