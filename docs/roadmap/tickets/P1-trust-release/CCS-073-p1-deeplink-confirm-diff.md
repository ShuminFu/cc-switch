# CCS-073: [deeplink] Show a full import preview with a risk diff in the confirm dialog

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P1`, `p2-medium`, `area:deeplink`

> **Ticket key:** `p1-deeplink-confirm-diff` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p1-deeplink-env-allowlist`, `p1-deeplink-script-disabled`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
This combines backend idea 2, frontend idea 4 and the plan backlog's "import preview with trust indicator". Users approve deeplinks without seeing the env keys, MCP commands or usage-script code that will be written, or how they differ from an existing provider with the same name.

## Proposed change
- Reuse merge_deeplink_config to return a structured preview: env keys (flagged by the allow-list), MCP server commands, the usage-script source and target host, and the live file(s) that will change.
- Render a diff against the existing provider with the same name, and add a "replaced by a newer link" banner.
- Mark risky items in red and require an extra checkbox before Import is enabled.

## Acceptance criteria / verification
- [x] RTL test: a link with an MCP command and a script renders both in full; Import stays disabled until the risk box is checked
- [x] `pnpm test:unit`, `pnpm typecheck`, `cargo test --manifest-path src-tauri/Cargo.toml deeplink`

## Dependencies
p1-deeplink-env-allowlist, p1-deeplink-script-disabled

## Implementation notes

**Summary.** A new Tauri command `preview_deeplink_import`
(`src-tauri/src/deeplink/preview.rs`) reuses `parse_and_merge_config` (the
code behind `merge_deeplink_config`, which is kept unchanged for
compatibility) and returns a structured `DeepLinkImportPreview`:

- `request`: the merged request (same as `merge_deeplink_config`);
- `unlistedEnvKeys` / `deniedEnvKeys`: Claude env keys classified with the
  CCS-070 allow-list;
- `mcpServers`: every server of an `mcp` link with its command, args, URL,
  full spec, and `exists` (an installed server only gets app flags merged,
  so its spec is not written);
- `usageScript`: the full decoded source and its target hosts (hosts of
  literal `http(s)://` URLs plus the host substituted for `{{baseUrl}}`,
  which is `usageBaseUrl` or the primary endpoint);
- `liveFiles` + `writesLiveOnImport`: the live config files the provider
  (or MCP sync) writes, and whether the import itself writes them. This
  mirrors `import_provider_from_deeplink`: additive apps always write, the
  others only when the provider becomes current, which unlisted env keys
  prevent;
- `existing`: the same-app provider with the same name, whether it is
  current, and a field-level diff (`settingsConfig` flattened to dotted
  paths, `websiteUrl`, `notes`, `icon`, `usageScript.*`) against the
  provider the link would create;
- `risks`: `unlistedEnv`, `mcpCommand` (a new server that launches a local
  command), `usageScript`.

`DeepLinkImportDialog` calls the preview for provider and MCP links instead
of `mergeDeeplinkConfig` (prompt/skill links keep the old merge path) and:

- renders MCP commands with all args, env (masked) and headers (always
  masked) in full, with no truncation; command servers are red;
- renders the usage-script source in full plus its target hosts in a red
  box. The status badge now always says "Disabled", matching CCS-069, which
  stores deeplink scripts disabled even when the link says
  `usageEnabled=true` (the old badge said "Enabled");
- renders the same-name diff with secrets masked by the leaf key of the
  field path (`maskFieldValue`; `maskConfigText` masks `key = value` lines
  inside multi-line TOML/YAML values), and the live files list;
- shows a risk checkbox (`deeplink-risk-ack`) whenever any risk is present.
  Import is disabled, and `handleImport` returns early, until the box is
  checked. The checkbox resets for every new request. If the preview call
  fails, the gate is still derived locally (unlisted env keys, any usage
  script, MCP servers with a `command`);
- "replaced by a newer link" banner: CCS-071 deliberately keeps the
  reviewed request when another link arrives while the dialog is open, so
  the newer link is not swapped in. Instead the open dialog now shows a
  banner (`deeplink-newer-link-banner`) saying a newer link arrived and was
  not applied, so the user knows to cancel and reopen it. This keeps the
  CCS-071 guarantee that the user never confirms content they did not see.

The import itself does not replace a same-name provider (it always creates
a new id), so the diff is informational and says the existing provider is
left unchanged.

New i18n keys: `deeplink.preview.*` and `deeplink.mcp.{command,url,env,headers,commandWarning,existsNote}`
in en / zh / zh-TW / ja.

**Files.**
- `src-tauri/src/deeplink/preview.rs` (new), `src-tauri/src/deeplink/mod.rs`,
  `src-tauri/src/commands/deeplink.rs`, `src-tauri/src/lib.rs`,
  `src-tauri/src/deeplink/tests.rs`
- `src/lib/api/deeplink.ts`, `src/lib/deeplink/mask.ts`,
  `src/components/DeepLinkImportDialog.tsx`,
  `src/components/deeplink/McpConfirmation.tsx`,
  `src/components/deeplink/ImportPreviewSections.tsx` (new)
- `src/i18n/locales/{en,zh,zh-TW,ja}.json`
- `tests/components/DeepLinkImportDialog.test.tsx`, `tests/lib/deeplinkMask.test.ts`

**Commits.** `26ef2fe` (feature), plus the commit that records these notes.

**Verification.**
- New RTL tests in `tests/components/DeepLinkImportDialog.test.tsx`:
  (1) an MCP link with an `npx` command and a 190-char arg renders the full
  command line, masks the env token, lists the live file, and Import stays
  disabled (a click imports nothing) until the risk box is checked;
  (2) a provider link with a usage script renders the exact full source,
  both target hosts and the "Disabled" badge, and Import stays disabled until
  the box is checked; (3) when the preview call fails, a locally parsed MCP
  command still gates Import; (4) a link with no risks shows no checkbox;
  (5) the same-name diff renders before/after values with tokens masked to 4
  characters; (6) the newer-link banner appears only in the dialog that
  ignored the link.
- Regression check: with the previous `DeepLinkImportDialog.tsx` and
  `McpConfirmation.tsx`, tests (1), (2), (3), (5) and (6) fail (5 failed /
  8 passed); with the change, 13/13 pass.
- Rust tests in `src-tauri/src/deeplink/tests.rs` (in-memory DB, isolated
  HOME): a provider preview flags `CUSTOM_FLAG` as unlisted and
  `NODE_OPTIONS` as denied, returns the full script with hosts
  `[new.relay.example, collector.example.net]`, reports no live write
  because of the unlisted key, and diffs base URL, token, the new env key and
  the script against the seeded same-name provider (denied keys are not in
  the diff); a plain Codex preview has no risks and writes auth.json and
  config.toml on import; an MCP preview reports `mcpCommand`, full args,
  `exists` for an installed server, and the URL of an http server. Plus unit
  tests for host extraction and flattening in `preview.rs`.
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml deeplink`: 50 passed.
  `cargo test --lib`: 2183 passed, 0 failed, 3 ignored.
- `pnpm typecheck`: clean. `pnpm test:unit`: 123 files, 1263 tests passed.
  `pnpm format:check`: all files use Prettier code style.
