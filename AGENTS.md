# AGENTS.md

Rules for anyone changing this repository: human contributors, coding agents, and the automated
PR reviewer (`.github/workflows/claude.yml` quotes this file). Keep it short; details live in the
linked pages.

## Read first

- [docs/dev/architecture.md](docs/dev/architecture.md): module map, data flow, invariants.
- [docs/dev/testing.md](docs/dev/testing.md): test layout and how to run what CI runs.
- [CONTRIBUTING.md](CONTRIBUTING.md): setup, PR rules, AI-assisted contribution policy.

## Before you open a PR

```bash
pnpm check        # typecheck, lint, format, i18n, repo checks, unit tests
pnpm check:rust   # cargo fmt, clippy --all-targets --all-features -D warnings, cargo test --locked
```

## Must

1. Keep SQLite (`~/.cc-switch/cc-switch.db`) the source of truth; live tool configs are a
   projection written through `src-tauri/src/adapters/`.
2. Never overwrite a live config file that failed to parse; back up before replacing; write
   atomically; preserve keys, comments and ordering the app does not own.
3. Schema changes only as a new migration in `src-tauri/src/database/schema.rs` (`MIGRATIONS`),
   with a test. Never edit the baseline DDL.
4. Every user-facing string goes through `t()` with the key added to all four locale files
   (`src/i18n/locales/{en,zh,zh-TW,ja}.json`).
5. TanStack Query keys only in `src/lib/query/keys.ts`. New IPC commands need a default MSW handler
   in `tests/msw/handlers.ts`.
6. `src/lib/bindings.ts` is generated: change the Rust command and run `pnpm gen:bindings`.
7. Never log secrets (API keys, tokens, deep-link query values) at `info` level or above.
8. Every bug fix comes with a regression test. Do not skip or delete existing tests.
9. Code must work on Windows, macOS and Linux (x64 and ARM64).
10. Commits and PR titles follow Conventional Commits (`feat(scope): …`, `fix(scope): …`).

## Must not

- Add a dependency, telemetry, or a new network request without saying why in the PR (and
  updating `PRIVACY.md` for a new request).
- Reformat or refactor code unrelated to the change.
- Commit secrets, real API keys or personal config files.
