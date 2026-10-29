# CCS-078: [security] Remove the unused asset protocol and protocol-asset feature

**Status:** Partial · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:security`

> **Ticket key:** `p1-asset-protocol-remove` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-unused-asset-protocol, partially real. The asset protocol is enabled with an empty scope and is never used. This is dead config.

## Evidence
- src-tauri/tauri.conf.json:30-33; src-tauri/Cargo.toml:30 protocol-asset; no convertFileSrc or asset:// anywhere in src/

## Proposed change
Remove both together, because tauri-build checks that they match.

## Acceptance criteria / verification
- [x] `cargo check --manifest-path src-tauri/Cargo.toml` passes
- [ ] `grep -rn 'convertFileSrc\|asset://' src` finds nothing; icons still render in `pnpm tauri dev`

## Dependencies
None.

## Implementation notes
- **Summary:** Removed `app.security.assetProtocol` from `src-tauri/tauri.conf.json` and the `protocol-asset` feature from the `tauri` dependency in `src-tauri/Cargo.toml`, together (tauri-build checks they agree). `Cargo.lock` loses the now-unused `http-range` crate. `tauri.windows.conf.json` never set the asset protocol. Added a vitest guard, `tests/lint/assetProtocol.test.ts`: no tauri config enables `assetProtocol`, the `tauri` dependency has no `protocol-asset`, and no file under `src/` uses `convertFileSrc` or `asset://`. The first two assertions fail against the previous config.
- **Files:** `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `tests/lint/assetProtocol.test.ts`.
- **Commits:** `690b0d8` (code + test); this ticket update follows in a separate commit.
- **Verification:**
  - `cargo check --manifest-path src-tauri/Cargo.toml`: OK.
  - `cargo fmt --check`: OK. `cargo clippy -- -D warnings`: OK.
  - `cargo test --lib`: 2215 passed, 0 failed, 3 ignored.
  - `pnpm exec vitest run tests/lint/assetProtocol.test.ts tests/lint/csp.test.ts`: 7/7 passed. `pnpm typecheck`: OK. Prettier check on the touched files: OK.
  - `git grep -n 'convertFileSrc\|asset://' -- src`: no matches.
- **Remaining:** The "icons still render in `pnpm tauri dev`" check needs someone to look at the app on a desktop session, which this headless environment can't do. The risk is low: icons come from bundled imports or `data:`/`https:` URLs, and none use `asset://`.
- **Review:** Confirmed both config and feature are removed together, no Rust code touches the asset scope, `tauri.windows.conf.json` has no override, and no file under `src/` references `convertFileSrc` or `asset://` (the only other hit is an ESLint fixture string in `tests/lint/eslintConfig.test.ts`). Follow-up: the `src/` scan in `tests/lint/assetProtocol.test.ts` only looked at `.ts`/`.tsx`, while the acceptance grep covers every file; it now also scans `.js`/`.jsx`/`.css`/`.html`/`.json`/`.svg`. Re-verified: full `cargo test --no-fail-fast` 2373 passed / 0 failed, `pnpm test:unit` 1387 passed, `pnpm typecheck` OK, Prettier OK. Status stays Partial until the icon check in `pnpm tauri dev` is done by hand.
