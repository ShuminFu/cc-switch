# Session manager

The session manager lists, searches, resumes, exports and deletes the
conversation transcripts that the supported CLIs write to disk. The backend
code is in `src-tauri/src/session_manager/` and the UI is in
`src/components/sessions/`. The Chinese PRD `session-manager.md` describes the
first macOS-only version and is kept as a historical document. This page
describes the current behaviour.

## Where each provider stores sessions

| Provider | Location (default) | Format | Resume command |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/projects/<project>/<id>.jsonl`, plus an optional sidecar directory `<id>/` (subagents, tool results) | JSONL, one event per line | `claude --resume <id>` |
| Codex | `~/.codex/sessions/**` and `~/.codex/archived_sessions/**`, with titles from `session_index.jsonl` | JSONL rollout files | `codex resume <id>` |
| Gemini CLI | `~/.gemini/tmp/<project>/chats/session-*.json` | One JSON document per session | `gemini --resume <id>` |
| Grok Build | `sessions/` and `archived_sessions/` in the Grok config directory, one directory per session (`chat_history.jsonl`, `summary.json`) | JSONL plus a JSON summary | `grok --resume <id>` |
| OpenCode | `$XDG_DATA_HOME/opencode` (default `~/.local/share/opencode`): legacy `storage/` JSON files and the newer `opencode.db` | JSON files and SQLite | `opencode -s <id>` |
| OpenClaw | `agents/<agent>/sessions/` in the OpenClaw directory | JSONL | none |
| Hermes | `state.db` and `sessions/` in the Hermes directory | SQLite and JSONL | none |

Each provider module (`providers/<name>.rs`) implements `scan_sessions`,
`load_messages` and `delete_session`. `registry.rs` lists the providers and
validates that a `source_path` the webview sends is inside that provider's
roots (`validate_source`) before anything reads or deletes it.

## Index, search and cache

`index.rs` keeps a derived SQLite cache in `~/.cc-switch/session-index.db`:
per-file parse results keyed by `(mtime, size)`, the last session list (served
on a cold start) and an FTS5 trigram index over titles and, after the first
transcript search, the transcript text. API keys and tokens are redacted
before indexing. The file can be deleted at any time and is rebuilt.

## Deletion and the trash

Deleting a file-based session moves its files (for Claude, also the sidecar
directory) to `~/.cc-switch/session-trash/<entry>/` with a `manifest.json`.
Entries are purged after 30 days. The delete toast offers **Undo**, and the
**Recently deleted** dialog (history icon in the session list header) lists
the entries and restores one to its original path. Sessions stored in SQLite
(OpenCode `opencode.db`, Hermes `state.db`) are still deleted directly and
cannot be restored yet.

## Pin, archive and tags

The pin, archive and tags on a session are user data, so they are not kept in
the rebuildable index. They are stored in `~/.cc-switch/session-annotations.json`,
keyed by the session key `provider:session:source`. The file is written
atomically, and a file that fails to parse is never overwritten. Pinned
sessions are listed first. Archived sessions are hidden until you turn on
**Show archived**. Tags are shown on the list item. The backend supports up to
20 tags of 40 characters, but the UI cannot edit tags yet.

Commands: `get_session_annotations`, `set_session_annotation(sessionKey,
annotation)`.

## Deep links

The `ccswitch://` protocol, including its parameters, encoding rules and size
limits, is specified in [../deeplink-spec.md](../deeplink-spec.md).
