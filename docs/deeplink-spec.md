# `ccswitch://` Deep Link Specification (v1)

This is the reference for the `ccswitch://` URL scheme that CC Switch registers on install. It is written for people who publish "Import into CC Switch" links (provider vendors, MCP and skill authors, team admins) and for contributors who change the parser.

The source of truth is the code. This document describes it as of v3.17.0:

| Concern | Code |
| --- | --- |
| URL grammar, required parameters, app validation | `src-tauri/src/deeplink/parser.rs` |
| Request fields | `DeepLinkImportRequest` in `src-tauri/src/deeplink/mod.rs` |
| Base64 decoding, URL validation, homepage inference | `src-tauri/src/deeplink/utils.rs` |
| Provider import and `config` merge | `src-tauri/src/deeplink/provider.rs` |
| Prompt / MCP / skill import | `src-tauri/src/deeplink/{prompt,mcp,skill}.rs` |
| OS entry point and log redaction | `handle_deeplink_url` in `src-tauri/src/lib.rs` |
| Tests | `src-tauri/src/deeplink/tests.rs`, `src-tauri/tests/deeplink_import.rs` |

If you change any of those files in a way that adds, removes or renames a resource, an app id or a parameter, update this document in the same PR.

The user manual has a shorter, task-oriented page: [5.3 Deep Link Protocol](user-manual/en/5-faq/5.3-deeplink.md).

## 1. Grammar

```
ccswitch://v1/import?resource=<resource>&<param>=<value>[&...]
```

| Part | Rule | Error if violated |
| --- | --- | --- |
| Scheme | Exactly `ccswitch` | `Invalid scheme: expected 'ccswitch', got '...'` |
| Host | The protocol version. Only `v1` is accepted. | `Missing version in URL host` / `Unsupported protocol version: ...` |
| Path | Exactly `/import` (no trailing slash) | `Invalid path: expected '/import', got '...'` |
| Query | `application/x-www-form-urlencoded` pairs, decoded with the `url` crate (`+` becomes a space, `%XX` is decoded) | — |
| `resource` | One of `provider`, `prompt`, `mcp`, `skill` | `Missing 'resource' parameter` / `Unsupported resource type: ...` |

Other rules that apply to every resource:

- **Parameter names are case-sensitive** and camelCase (`apiKey`, not `api_key` or `apikey`).
- **Unknown parameters are ignored.** Adding a parameter is therefore backward compatible for old app versions, but a link cannot detect that an old version ignored it.
- **Duplicate parameters**: the last occurrence wins.
- **Booleans** (`enabled`, `usageEnabled`) must be exactly `true` or `false`. Any other value (`1`, `yes`, `True`) is silently treated as "not given".
- **Integers** (`usageAutoInterval`) must parse as an unsigned 64-bit integer; anything else is treated as "not given".
- There is **no version negotiation** beyond the `v1` host and no maximum URL length enforced by CC Switch. Operating systems and browsers impose their own limits (Windows is the tightest, on the order of a few thousand characters for the command line), so keep inline `config` payloads small.
- Only the **first** `ccswitch://` URL passed on the command line is processed.

## 2. Encoding

1. Every value must be **URL-encoded** (`encodeURIComponent` in JavaScript, `urllib.parse.quote(..., safe="")` in Python). This matters most for `name`, `endpoint`, `notes` and Base64 values.
2. Parameters marked **Base64** below carry UTF-8 text encoded as Base64, and then URL-encoded. The decoder (`decode_base64_param`) is lenient: it accepts standard and URL-safe alphabets, with or without `=` padding, and turns spaces back into `+` (in case `+` was not escaped). The safest choice is URL-safe Base64 without padding, or standard Base64 with `+`, `/` and `=` percent-encoded.
3. Decoded Base64 content must be valid UTF-8.

## 3. App ids

| App id | `provider` | `prompt` | `mcp` (`apps=`) | Notes |
| --- | :-: | :-: | :-: | --- |
| `claude` | yes | yes | yes | Claude Code |
| `codex` | yes | yes | yes | |
| `gemini` | yes | yes | yes | Gemini CLI |
| `grokbuild` | yes | yes | yes | Grok Build. `grok` is also accepted, but only inside `apps=` for MCP. |
| `opencode` | yes | yes | yes | |
| `openclaw` | yes | yes | accepted, ignored | OpenClaw has no MCP support; the id is accepted in `apps=` and silently dropped. |
| `hermes` | yes | yes | yes | |

Claude Desktop (`claude-desktop`) is **not** a valid deep-link target in v1. Skills always target Claude (see §7).

## 4. `resource=provider`

Adds a provider to the chosen app. With `enabled=true` it also switches to it.

| Parameter | Required | Type | Description |
| --- | --- | --- | --- |
| `app` | **yes** | app id | One of the seven ids in §3. |
| `name` | **yes** | string | Display name. Also used for the provider id: `<name with only [A-Za-z0-9_-] kept, lowercased>-<unix ms>`. |
| `endpoint` | yes\* | URL list | API base URL. Several URLs may be comma-separated: the first becomes the provider's base URL, the rest are added as custom endpoints (trailing `/` removed). Each must be `http` or `https`. |
| `apiKey` | yes\* | string | API key. Must not be empty after merging. |
| `homepage` | no | URL | Provider website (`http`/`https`). If omitted it is inferred from the first endpoint by stripping a leading `api.` or `api-` from the host, e.g. `https://api.example.com/v1` → `https://example.com`. |
| `model` | no | string | Default model. Codex defaults to `gpt-5-codex`, Grok Build to `grok-4.5`. |
| `haikuModel` | no | string | Claude only: `ANTHROPIC_DEFAULT_HAIKU_MODEL`. |
| `sonnetModel` | no | string | Claude only: `ANTHROPIC_DEFAULT_SONNET_MODEL`. |
| `opusModel` | no | string | Claude only: `ANTHROPIC_DEFAULT_OPUS_MODEL`. |
| `notes` | no | string | Free-text notes shown on the provider card. |
| `icon` | no | string | Built-in icon name; trimmed and lowercased. |
| `enabled` | no | boolean | `true` switches to the new provider after import (writes the app's live config). Default `false`. |
| `config` | no | Base64 | Full config file, merged as described in §4.1. |
| `configFormat` | no | `json` \| `toml` | Format of `config`. Default `json`. Any other value is an error. |
| `configUrl` | no | URL | **Reserved, not implemented.** If present without `config`, the import fails with `Remote config URL is not yet supported. Use inline config instead.` |
| `usageScript` | no | Base64 | JavaScript usage-query script. |
| `usageEnabled` | no | boolean | Enable usage queries. Default: `true` if `usageScript` decodes to non-empty code, otherwise `false`. |
| `usageApiKey` | no | string | API key for usage queries, if different from `apiKey` (dropped when equal). |
| `usageBaseUrl` | no | string | Base URL for usage queries, if different from the first endpoint (dropped when equal, ignoring trailing `/`). |
| `usageAccessToken` | no | string | Access token (NewAPI-style templates). |
| `usageUserId` | no | string | User id (NewAPI-style templates). |
| `usageAutoInterval` | no | integer | Automatic usage refresh interval in minutes; `0` disables it. |

\* `endpoint` and `apiKey` are optional in the URL but **required after merging**: they must come from either the URL or `config`, or the import fails with `Endpoint is required (either in URL or config file)` / `API key is required (either in URL or config file)`.

If any `usage*` parameter is present, the provider gets a usage script with language `javascript` and a 10-second timeout.

### 4.1 Merging `config`

**URL parameters always win.** Values from `config` only fill in fields the URL left empty. The expected `config` shape and the fields read from it depend on the app:

| App | Expected `config` (after Base64 decoding) | Fields read |
| --- | --- | --- |
| `claude` | JSON with an `env` object, as in `~/.claude/settings.json`. Missing `env` is an error. | `ANTHROPIC_AUTH_TOKEN` → `apiKey`, `ANTHROPIC_BASE_URL` → `endpoint`, `ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS}_MODEL`. Every other key in `env` (for example `API_TIMEOUT_MS`, `ANTHROPIC_CUSTOM_HEADERS`) is kept as-is in the imported provider. |
| `codex` | JSON `{"auth": {...}, "config": "<config.toml text>"}` | API key from `auth.OPENAI_API_KEY` (or the bearer token Codex itself accepts); `base_url` from the first `[model_providers.*]` table; top-level `model`. |
| `gemini` | Flat JSON object (the `.env` keys) | `GEMINI_API_KEY`, `GOOGLE_GEMINI_BASE_URL` (or `GEMINI_BASE_URL`), `GEMINI_MODEL`. |
| `grokbuild` | JSON `{"config": "<config.toml text>"}`, or the TOML itself (with `configFormat=toml`) | The default model profile: `model`, `base_url`, `api_key`. The config must contain a valid model profile. |
| `opencode`, `openclaw`, `hermes` | JSON object | `apiKey` or `api_key`; `baseUrl`, `base_url` or `options.baseURL`. |

With `configFormat=toml`, the TOML is converted to JSON first and then read with the same rules.

### 4.2 What gets written

The imported provider's settings are generated from the merged values, not copied verbatim from `config` (except the extra Claude `env` keys noted above):

| App | Generated settings |
| --- | --- |
| `claude` | `{"env": {ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL, [ANTHROPIC_MODEL], [ANTHROPIC_DEFAULT_*_MODEL], ...extra env}}` |
| `codex` | `auth.OPENAI_API_KEY` plus a `config.toml` with `model_provider = "custom"`, `model`, `model_reasoning_effort = "high"`, `disable_response_storage = true` and a `[model_providers.custom]` table (`name` = link `name`, `base_url`, `wire_api = "responses"`, `requires_openai_auth = true`). |
| `gemini` | `{"env": {GEMINI_API_KEY, GOOGLE_GEMINI_BASE_URL, [GEMINI_MODEL]}}` |
| `grokbuild` | A `config.toml` with `[models] default` and one `[model."<model>"]` profile (`base_url`, `name`, `api_key`, `api_backend = "responses"`, `context_window = 500000`). |
| `opencode` | `{"npm": "@ai-sdk/openai-compatible", "options": {baseURL, apiKey}, "models": {<model>: {name}}}` |
| `openclaw` | `{"baseUrl", "apiKey", "api": "openai-completions", "models": [{id, name}]}` |
| `hermes` | `{"name", "base_url", "api_key", "api_mode": "chat_completions", "models": [{id, name}]}` |

## 5. `resource=prompt`

Adds a prompt preset for one app.

| Parameter | Required | Type | Description |
| --- | --- | --- | --- |
| `app` | **yes** | app id | One of the seven ids in §3. |
| `name` | **yes** | string | Prompt name (also used for the id, as for providers). |
| `content` | **yes** | Base64 | Markdown content. |
| `description` | no | string | Short description. |
| `enabled` | no | boolean | `true` activates the prompt after import, which deactivates the app's other prompts and writes the app's prompt file. Default `false`. |

## 6. `resource=mcp`

Imports one or more MCP servers.

| Parameter | Required | Type | Description |
| --- | --- | --- | --- |
| `apps` | **yes** | comma list | Apps to enable the servers for, e.g. `claude,codex,gemini`. Spaces around ids are allowed. Every id must be in the `mcp` column of §3, and at least one must be an MCP-capable app. |
| `config` | **yes** | Base64 | JSON of the form `{"mcpServers": {"<id>": <server spec>, ...}}` (the Claude Desktop / Claude Code format). The object must not be empty. The config is always JSON; `configFormat` is ignored. |
| `enabled` | no | boolean | Parsed but currently **has no effect**: servers are always enabled for the apps in `apps`. |

Behaviour per server id:

- **New id**: created with the given spec, `name` = id, tag `imported`, enabled for `apps`.
- **Existing id**: the stored spec is **kept unchanged**; only the apps in `apps` are additionally enabled. A link cannot overwrite an existing server's command or credentials.
- Servers that fail to save are reported individually; the others are still imported.

## 7. `resource=skill`

Adds a GitHub repository to the skill repositories list. It does not install any skill by itself; the user installs skills from that repository in the Skills panel.

| Parameter | Required | Type | Description |
| --- | --- | --- | --- |
| `repo` | **yes** | `owner/name` | GitHub repository. Exactly one `/`. |
| `branch` | no | string | Branch to read. Default `main`. |
| `directory` | no | string | Parsed and shown in the confirmation dialog, but **not stored** by the importer in v3.17.0. |

`app` and `name` are ignored for skills; the request is always treated as `app=claude`. The repository is saved as enabled.

## 8. User confirmation and security model

- Parsing happens in the Rust backend. A parse error is shown to the user and nothing is imported.
- A successful parse never imports directly. The app opens and shows a confirmation dialog with the parsed (and, for providers, merged) values; nothing is written until the user clicks Import.
- At `info` level only the scheme, version, path and parameter names are logged; the full URL, including any key, is logged at `debug` level, so debug logs from a deep-link import should be treated as secret.
- `endpoint` and `homepage` must be `http` or `https`. No network request is made during import (`configUrl` is not fetched).
- A deep link can carry an API key. Anyone who has the link has the key, so treat links with `apiKey`, `usageApiKey`, `usageAccessToken` or a `config` containing keys as secrets. Do not publish them, and do not commit them to documentation or examples (use obvious placeholders such as `YOUR_API_KEY`).
- A provider link with `enabled=true` rewrites the app's live config once the user confirms. An MCP link can add servers that run local commands. Users should only import links from sources they trust.

## 9. Examples

Provider (Claude), minimal:

```
ccswitch://v1/import?resource=provider&app=claude&name=Example%20Relay&endpoint=https%3A%2F%2Fapi.example.com&apiKey=YOUR_API_KEY
```

Provider (Codex) with a model, switched to immediately:

```
ccswitch://v1/import?resource=provider&app=codex&name=Example&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=YOUR_API_KEY&model=gpt-5-codex&enabled=true
```

Provider (Claude) with two endpoints and model aliases:

```
ccswitch://v1/import?resource=provider&app=claude&name=Example&endpoint=https%3A%2F%2Fapi.example.com%2Chttps%3A%2F%2Fbackup.example.com&apiKey=YOUR_API_KEY&haikuModel=example-small&sonnetModel=example-medium&opusModel=example-large
```

MCP (one stdio server for Claude and Codex). The `config` value is the Base64 of `{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}`:

```
ccswitch://v1/import?resource=mcp&apps=claude,codex&config=eyJtY3BTZXJ2ZXJzIjp7ImZldGNoIjp7ImNvbW1hbmQiOiJ1dngiLCJhcmdzIjpbIm1jcC1zZXJ2ZXItZmV0Y2giXX19fQ
```

Prompt. The `content` value is the Base64 of `# Reviewer\nYou are a careful code reviewer.`:

```
ccswitch://v1/import?resource=prompt&app=claude&name=Reviewer&content=IyBSZXZpZXdlcgpZb3UgYXJlIGEgY2FyZWZ1bCBjb2RlIHJldmlld2VyLg
```

Skill repository:

```
ccswitch://v1/import?resource=skill&repo=owner%2Frepo&branch=main
```

Generating a link in JavaScript:

```javascript
const b64url = (text) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)))
    .replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");

const params = new URLSearchParams({
  resource: "mcp",
  apps: "claude,codex",
  config: b64url(JSON.stringify({ mcpServers: { fetch: { command: "uvx", args: ["mcp-server-fetch"] } } })),
});
const link = `ccswitch://v1/import?${params}`;
```

## 10. Compatibility

| Since | Change |
| --- | --- |
| v3.7.1 | `haikuModel`, `sonnetModel`, `opusModel` |
| v3.8 | `config`, `configFormat`, `configUrl` (reserved); `endpoint`/`apiKey`/`homepage` may come from `config` |
| v3.9 | `usage*` parameters |
| later 3.x | `opencode`, `openclaw`, `hermes`, `grokbuild` app ids |

Older app versions reject app ids they do not know (`Invalid app type`) and ignore parameters they do not know.

The online generator (`deplink.html`, published at `https://farion1231.github.io/cc-switch/deplink.html`) currently offers only `claude`, `codex` and `gemini`; links for the other apps have to be built by hand following this spec.
