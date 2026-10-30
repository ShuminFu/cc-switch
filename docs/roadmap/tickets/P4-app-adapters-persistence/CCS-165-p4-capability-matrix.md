# CCS-165: [settings] Add a capability matrix page

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:settings-ui`

> **Ticket key:** `p4-capability-matrix` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p2-fe-app-registry`, `p4-adapter-conformance`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Users find missing capabilities by trial and error.

## Proposed change
A settings page rendering APPS x caps (MCP, skills, prompts, takeover, usage, session resume) from get_app_registry. It doubles as a support diagnostics view, with a copy-to-clipboard action.

## Acceptance criteria / verification
- [x] RTL: the matrix renders for all 8 apps and matches the registry fixture
- [x] `pnpm test:unit`

## Dependencies
p2-fe-app-registry, p4-adapter-conformance

## Implementation notes

**Summary.** Settings > Advanced has a new "Capability Matrix" accordion
(`CapabilityMatrix`). It loads `get_app_registry` through react-query and
renders a table with one row per app (8 rows, in registry order) and one
column per capability: MCP, Skills, Prompts, Proxy takeover, Usage stats,
Session resume. Each cell shows an icon plus screen-reader text. A "Copy
diagnostics" button copies an English Markdown table
(`formatCapabilityMatrix`, with backend ids and field names so it reads the
same in any UI language) through `copyText`, and shows a toast on success
or failure.

The backend registry did not expose usage or session resume, so
`apps::registry::AppCaps` gains `USAGE` and `SESSION_RESUME`, and both are
added to `AppCapsDto` (`usage`, `sessionResume`):
- `USAGE` is set for apps with a proxy route (claude, claude-desktop, codex,
  gemini, grokbuild) or a session-log importer (opencode). A test ties it to
  `proxy_route` and the importer list in `sync_all_session_usage`.
- `SESSION_RESUME` is set for claude, codex, gemini, grokbuild and opencode. A
  test ties it to `session_manager::build_resume_command`.

`tests/fixtures/app-registry.json` is a full snapshot of
`get_app_registry`. The Rust test `registry_dto_matches_shared_fixture`
checks the DTO against it. The RTL test and the default MSW
`get_app_registry` handler serve the same file, so it was removed from
`UNMOCKED_COMMANDS`.

**Files.**
- `src-tauri/src/apps/registry.rs`
- `src/lib/api/apps.ts`
- `src/components/settings/CapabilityMatrix.tsx` (new)
- `src/components/settings/SettingsPage.tsx`
- `src/i18n/locales/{en,zh,zh-TW,ja}.json`
- `tests/fixtures/app-registry.json` (new)
- `tests/components/CapabilityMatrix.test.tsx` (new)
- `tests/msw/handlers.ts`
- `tests/msw/unmockedCommands.ts`

**Commits.** dfc90b3 (feature), plus the follow-up commit that records these notes.

**Verification.**
- `cargo test --lib apps::registry`: 17 passed. This includes the new
  `usage_cap_matches_proxy_routes_and_session_log_imports`,
  `session_resume_cap_matches_resume_command_templates` and
  `registry_dto_matches_shared_fixture`.
- `cargo test --lib`: 2431 passed, 0 failed, 3 ignored.
- `cargo clippy -- -D warnings`: clean. `cargo fmt`: clean.
- `pnpm typecheck`: clean. `pnpm format:check`: clean.
- `pnpm test:unit`: 158 files passed. `tests/components/CapabilityMatrix.test.tsx`
  has 6 tests: the fixture covers all 8 apps; rows and cells match the fixture;
  usage and session resume show for Claude Desktop; the copy text; copy failure;
  registry load failure.
