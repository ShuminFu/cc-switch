# CCS-164: [mcp] Add OpenClaw MCP support (Issue #4834)

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:mcp`

> **Ticket key:** `p4-openclaw-mcp` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-mcp-projector`, `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- A stub at services/mcp.rs:142

## Proposed change
Implement McpProjector for OpenClaw and set caps.mcp.

## Acceptance criteria / verification
- [x] Conformance MCP upsert and remove for OpenClaw
- [x] Writer snapshot for the OpenClaw MCP section
- [x] `cargo test --manifest-path src-tauri/Cargo.toml mcp`

## Dependencies
p4-mcp-projector, p4-adapter-additive

## Implementation notes

**Summary.** `OpenClawAdapter` now implements `McpProjector`, and the OpenClaw registry entry carries `AppCaps::MCP` (backend `apps/registry.rs` and frontend `src/entities/app/registry.ts`, so the MCP panel shows an OpenClaw toggle).
- **Format.** Servers go into the `mcp.servers` section of `~/.openclaw/openclaw.json`.
  - stdio is written as `command`/`args`/`env`/`cwd`.
  - Remote servers are written as `url`/`headers` plus `transport` (`"sse"`, or `"streamable-http"` for `type: "http"`).
  - On import, a `url` with no `transport` is read as SSE.
- **Writing.** Writes go through the existing round-trip JSON5 document (`write_root_section("mcp", …)`), so comments and keys outside `mcp` are kept byte for byte.
  - Upserts merge into the existing entry: CC Switch owns the connection fields, and any other per-server key (for example `connectionTimeoutMs`) is kept.
  - Removal keeps the order of the other servers.
  - An unchanged entry is not rewritten.
  - Sync is skipped when `~/.openclaw/` does not exist, the same as Hermes and OpenCode.
- **Other changes.**
  - `McpService::project_servers_to_app` now skips apps with no projector instead of naming OpenClaw explicitly.
  - Deep links now accept `apps=openclaw` for MCP; before, it was silently dropped.
  - The legacy v3.6 migration comment now says why OpenClaw is skipped.
- **Shared `AppSet`.** `AppSet` holds both MCP and Skills flags and accepts any app with either capability. OpenClaw has MCP but not Skills, so `AppJoinTable` gained a `cap`.
  - `mcp_server_apps` reads and writes only MCP-capable apps.
  - `skill_apps` reads and writes only Skills-capable apps.
  - Rows for apps outside a table's capability are kept untouched, as rows from newer builds already were.
  - For the apps that already existed, behaviour is identical, because they all have both capabilities.
- **User manual.** The en/ja/zh pages list OpenClaw as supported.

**Files.**
- New: `src-tauri/src/mcp/openclaw.rs` (conversion, merge-on-write, sync/remove/read, unit tests)
- `src-tauri/src/apps/openclaw/{config,mod}.rs` (`get_mcp_servers`, `update_mcp_servers`, the projector)
- `src-tauri/src/apps/registry.rs`, `src-tauri/src/app_config.rs`, `src-tauri/src/database/dao/app_sets.rs`
- `src-tauri/src/mcp/mod.rs`, `src-tauri/src/services/mcp.rs`, `src-tauri/src/deeplink/mcp.rs`
- `src-tauri/tests/config_writers.rs`, new `src-tauri/tests/fixtures/live/openclaw_mcp/{openclaw.json,servers.json,lf.snap,crlf.snap,bom.snap}`
- `src-tauri/tests/mcp_commands.rs`
- `src/entities/app/registry.ts`, `src/config/appConfig.tsx`, `tests/entities/appRegistry.test.tsx`
- `docs/user-manual/{en,ja,zh}/3-extensions/3.1-mcp.md`

**Tests.**
- **Conformance:** `app_adapter_conformance::mcp_upsert_and_remove_for_every_app` now runs on OpenClaw, because `mcp()` is `Some`; before this change OpenClaw was skipped. It checks upsert, re-upsert, remove, and that removing again is a no-op.
- **Writer snapshot:** new `config_writers::openclaw_mcp_json5` runs on LF, CRLF and BOM inputs. It upserts a new stdio server, re-upserts an existing SSE server as HTTP (which keeps `connectionTimeoutMs`), and removes a stale one. It asserts that the user's comment block and unknown root keys are preserved verbatim.
- **Service tests:**
  - New `toggle_openclaw_projects_server_into_openclaw_json`: enable and then disable through `McpService::toggle_app`; comments and user-owned servers survive.
  - `import_from_all_apps_loops_over_every_mcp_capable_app` now seeds OpenClaw (stdio plus streamable-http) and uses Claude Desktop as the app without a projector.
- **DAO tests:**
  - New `each_join_table_only_stores_apps_with_its_capability`.
  - The "unknown rows survive" test now uses an `openclaw` row in `skill_apps`.
- Registry and `AppSet` unit tests were updated for the new capability.

**Commits.** `8c9d9b0` (code, tests, docs); this ticket edit is in a follow-up `docs(roadmap)` commit.

**Verification.**
- `cargo fmt --check`: clean.
- `cargo clippy -- -D warnings` and `cargo clippy --tests -- -D warnings`: clean.
- `cargo test --test config_writers`: 19 passed.
- `cargo test --test mcp_commands --test app_adapter_conformance --test deeplink_import`: 22 + 13 + 4 passed.
- `cargo test mcp`: 81 passed, 0 failed.
- `cargo test --lib`: 2425 passed, 0 failed.
- `cargo test --no-fail-fast` (full suite): 2626 passed, 0 failed.
- `pnpm typecheck`: clean.
- `pnpm test:unit`: 157 files, 1532 tests passed.
- `pnpm format:check`: clean.

**Not verified against a real install.** OpenClaw's schema (`mcp.servers` with a `transport` of `sse` or `streamable-http`) follows OpenClaw's `openclaw mcp set` config shape. There is no OpenClaw binary in this environment, so a manual check that OpenClaw loads a projected server is still open.

**Review.** Checked every acceptance criterion against the code. The OpenClaw schema was compared with OpenClaw's own docs (`docs/cli/mcp/transports.md`): `mcp.servers.<name>` with `command`/`args`/`env`/`cwd` or `url`/`headers`/`transport` (`sse` default, `streamable-http`) matches. Follow-up fixes in `src-tauri/src/mcp/openclaw.rs`:
- The upsert merge moved the user's extra keys (e.g. `connectionTimeoutMs`) to the top of the entry. It now keeps the entry's key order: existing keys stay in place, and new connection fields are appended.
- OpenClaw also accepts `workingDirectory` for `cwd` and the CLI-native `type` for `transport`. These aliases are now connection fields CC Switch owns, so a stale alias is dropped on write and cannot conflict with the `cwd`/`transport` CC Switch writes. On import, `workingDirectory` is read as `cwd` when `cwd` is missing.
- New unit tests `merge_keeps_the_user_key_order`, `merge_drops_stale_aliases_of_core_fields` and `working_directory_alias_imports_as_cwd`. The `openclaw_mcp` snapshots were updated for the key order.
- Verification: `cargo fmt --check` and `cargo clippy (--tests) -- -D warnings` clean; full `cargo test --no-fail-fast` 2629 passed, 0 failed; `pnpm typecheck` clean.
