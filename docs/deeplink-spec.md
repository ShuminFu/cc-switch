# `ccswitch://` Deep Link Specification (v1 and v2)

This is the reference for the `ccswitch://` URL scheme that CC Switch registers on install. It is written for people who publish "Import into CC Switch" links (provider vendors, MCP and skill authors, team admins) and for contributors who change the parser.

The source of truth is the code. This document describes v1 as of v3.17.0 plus the changes made since (v2 bundles and signatures, the backend-held pending import, size limits). §11 covers v2.

| Concern | Code |
| --- | --- |
| URL grammar, required parameters, app validation | `src-tauri/src/deeplink/parser.rs` |
| Request fields | `DeepLinkImportRequest` in `src-tauri/src/deeplink/mod.rs` |
| Base64 decoding, URL validation, homepage inference | `src-tauri/src/deeplink/utils.rs` |
| Provider import and `config` merge | `src-tauri/src/deeplink/provider.rs` |
| Prompt / MCP / skill import | `src-tauri/src/deeplink/{prompt,mcp,skill}.rs` |
| v2 envelope, signatures, publisher keys | `src-tauri/src/deeplink/{v2,trust}.rs`, `verify_minisign` in `src-tauri/src/services/catalog.rs` |
| Pending imports (nonce), bundle import, share links | `src-tauri/src/commands/deeplink.rs` |
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
| Host | The protocol version: `v1` (this grammar) or `v2` (§11). A higher `vN` is answered with "needs a newer CC Switch". | `Missing version in URL host` / `Unsupported protocol version: ...` |
| Path | Exactly `/import` (no trailing slash) | `Invalid path: expected '/import', got '...'` |
| Query | `application/x-www-form-urlencoded` pairs, decoded with the `url` crate (`+` becomes a space, `%XX` is decoded) | — |
| `resource` | One of `provider`, `prompt`, `mcp`, `skill` | `Missing 'resource' parameter` / `Unsupported resource type: ...` |

Other rules that apply to every resource:

- **Parameter names are case-sensitive** and camelCase (`apiKey`, not `api_key` or `apikey`).
- **Unknown parameters are ignored.** Adding a parameter is therefore backward compatible for old app versions, but a link cannot detect that an old version ignored it.
- **Duplicate parameters**: the last occurrence wins.
- **Booleans** (`enabled`, `usageEnabled`) must be exactly `true` or `false`. Any other value (`1`, `yes`, `True`) is silently treated as "not given".
- **Integers** (`usageAutoInterval`) must parse as an unsigned 64-bit integer; anything else is treated as "not given".
- **Limits**: a URL longer than 64 KiB is rejected before parsing; a Base64 parameter may decode to at most 256 KiB; one MCP link may carry at most 50 servers. Operating systems and browsers impose tighter limits of their own (Windows is on the order of a few thousand characters for the command line), so keep inline `config` payloads small.
- **Version negotiation**: v2 links can declare `minApp` (§11); an older app that sees `v3` or later tells the user to update.
- Only the **first** `ccswitch://` URL passed on the command line is processed.

## 2. Encoding

1. Every value must be **URL-encoded** (`encodeURIComponent` in JavaScript, `urllib.parse.quote(..., safe="")` in Python). This matters most for `name`, `endpoint`, `notes` and Base64 values.
2. Parameters marked **Base64** below carry UTF-8 text encoded as Base64, and then URL-encoded. The decoder (`decode_base64_param`) normalises once and decodes once: spaces become `+` again (in case `+` was not escaped), the URL-safe `-`/`_` map to `+`/`/`, and missing `=` padding is added. The safest choice is URL-safe Base64 without padding, or standard Base64 with `+`, `/` and `=` percent-encoded.
3. Decoded Base64 content must be valid UTF-8.

## 3. App ids

| App id | `provider` | `prompt` | `mcp` (`apps=`) | Notes |
| --- | :-: | :-: | :-: | --- |
| `claude` | yes | yes | yes | Claude Code |
| `codex` | yes | yes | yes | |
| `gemini` | yes | yes | yes | Gemini CLI |
| `grokbuild` | yes | yes | yes | Grok Build. The alias `grok` is accepted for every resource and normalised to `grokbuild`. |
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
| `configUrl` | no | URL | **Not supported.** A link with `configUrl` and no `config` is rejected when it is opened (no confirmation dialog); with `config` present, `configUrl` is ignored. |
| `usageScript` | no | Base64 | JavaScript usage-query script. |
| `usageEnabled` | no | boolean | Requested only. The script is enabled only if the user ticks the option in the dialog (which shows the script source); default off. |
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
| `enabled` | no | boolean | Requested only. Servers are written into the apps' live configs only if the user ticks "enable now" in the dialog; otherwise they are stored with every app off. |

Endpoints taken from an inline `config` (e.g. `env.ANTHROPIC_BASE_URL`, a TOML `base_url`, `options.baseURL`) are validated like URL parameters: anything but `http`/`https` fails both the preview and the import.

Behaviour per server id:

- **New id**: created with the given spec, `name` = id, tag `imported`, enabled for `apps`.
- **Existing id**: the stored spec is **kept unchanged**; only the apps in `apps` are additionally enabled. A link cannot overwrite an existing server's command or credentials.
- Servers that fail to save are reported individually; the others are still imported.

## 7. `resource=skill`

Adds a GitHub repository to the skill repositories list. It does not install any skill by itself; the user installs skills from that repository in the Skills panel.

| Parameter | Required | Type | Description |
| --- | --- | --- | --- |
| `repo` | **yes** | `owner/name` | GitHub repository. Exactly one `/`; owner and name are 1-100 of `[A-Za-z0-9_.-]` and not `.`/`..`. |
| `branch` | no | string | Branch to read. Default `main`. 1-255 of `[A-Za-z0-9._/-]`, no `..`, not starting with `/` or `-`. |
| `directory` | no | string | **Ignored.** It was never honoured by the importer and is no longer shown. |

`app` and `name` are ignored for skills; the request is always treated as `app=claude`. The repository is saved as enabled.

## 8. User confirmation and security model

- Parsing happens in the Rust backend. A parse error is shown to the user and nothing is imported.
- **The backend keeps the link.** Every link the operating system delivers is stored as a *pending import* under a random nonce; the request shown in the dialog carries that nonce. The import commands (`import_from_deeplink_unified`, `import_deeplink_bundle`) only accept a nonce of a pending link, take the data from the backend's copy, and accept only the user's yes/no choices from the dialog. A request fabricated in the webview (for example from devtools) is rejected. A nonce is single-use and expires after 30 minutes; cancelling the dialog discards it.
- A successful parse never imports directly. The dialog shows the parsed (and, for providers, merged) values; nothing is written until the user clicks Import, which is armed shortly after the content appears. Links that arrive while a dialog is open are queued and shown one after another; they never replace the request under review.
- **`enabled` is a request, not a decision.** It means "activate after import" for every resource and is always off unless the user ticks the matching option:

  | Resource | What activation does | Default |
  | --- | --- | --- |
  | `provider` | Makes the provider current (writes the app's live config). | off |
  | `prompt` | Enables the prompt and replaces the live `CLAUDE.md` / `AGENTS.md`; the current live file is first saved as a backup prompt. | off |
  | `mcp` | Writes the servers into the apps' live configs. | off |
  | `skill` | The repository is listed as an enabled skill source (nothing is installed). | on |

- The dialog shows the full prompt text, every MCP command/argument (secret-looking env values masked) and the usage-script source.
- Every link is labelled: v1 links and unsigned v2 links as **unsigned**; signed v2 links with the publisher and key id (§11.3).
- Logs never contain the URL's query values (only parameter names), and the `deeplink-error` event carries the redacted URL only.
- `endpoint` and `homepage` must be `http` or `https`. No network request is made during import (`configUrl` is not fetched).
- A deep link can carry an API key. Anyone who has the link has the key, so treat links with `apiKey`, `usageApiKey`, `usageAccessToken` or a `config` containing keys as secrets. Do not publish them, and do not commit them to documentation or examples (use obvious placeholders such as `YOUR_API_KEY`).

## 9. Examples

Provider (Claude), minimal:

```
ccswitch://v1/import?resource=provider&app=claude&name=Example%20Relay&endpoint=https%3A%2F%2Fapi.example.com&apiKey=YOUR_API_KEY
```

Provider (Codex) with a model, asking to be switched to (the user still has to tick the option):

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

The online generator (`deplink.html`) builds v1 links only. Configured providers, MCP servers and prompts can be shared from inside the app (`build_deeplink_share_link`, §11.4), which produces v2 links.

## 11. Version 2: bundles and signatures

### 11.1 Grammar

```
ccswitch://v2/import?p=<base64url(envelope JSON)>[&sig=<base64url(minisign signature file)>]
```

`p` and `sig` are URL-safe Base64 without padding (standard Base64 and padding are tolerated). The decoded envelope may be at most 32 KiB.

```json
{
  "v": 2,
  "items": [
    { "resource": "provider", "app": "claude", "name": "Example Relay",
      "endpoint": "https://api.example.com", "apiKey": "YOUR_API_KEY" },
    { "resource": "mcp", "apps": "claude,codex",
      "config": { "mcpServers": { "docs": { "command": "npx", "args": ["-y", "@example/docs-mcp"] } } } },
    { "resource": "prompt", "app": "claude", "name": "Guidelines", "content": "<Base64 Markdown>" }
  ],
  "publisher": "Example Vendor",
  "publisherKey": "RWS...",
  "minApp": "3.18.0"
}
```

| Field | Required | Description |
| --- | --- | --- |
| `v` | **yes** | Must be `2`. |
| `items` | **yes** | 1-16 resources. Each item uses exactly the v1 parameters of its `resource` (§4-§7) with the same validation. Strings, booleans and numbers are taken as is; an object or array value (typically `config`) is inline JSON and is Base64-encoded internally. |
| `publisher` | no | Name the publisher claims. Shown as unverified unless the key is known (§11.3). |
| `publisherKey` | with `sig` | The publisher's minisign public key (the `RW...` line of `minisign.pub`). |
| `minApp` | no | Lowest CC Switch version that can import the link; older apps ask the user to update. |

All items are shown in one dialog with a per-item accept toggle; activation (`enabled`, usage scripts) is chosen per item and always starts off.

### 11.2 Signing

The signature covers the exact decoded bytes of `p`. Use [minisign](https://jedisct1.github.io/minisign/) (the same tool and format as CC Switch's updater):

```sh
minisign -G -p publisher.pub -s publisher.key        # once; keep publisher.key private
# write the envelope with "publisherKey" set to the second line of publisher.pub
minisign -S -s publisher.key -m envelope.json -t "Example Vendor onboarding"
python3 - <<'PY'
import base64
enc = lambda b: base64.urlsafe_b64encode(b).decode().rstrip("=")
p = open("envelope.json", "rb").read()
sig = open("envelope.json.minisig", "rb").read()
print(f"ccswitch://v2/import?p={enc(p)}&sig={enc(sig)}")
PY
```

Rules enforced by the app:

- A link with `sig` must verify with its own `publisherKey` (prehashed minisign signature, trusted comment included). A tampered envelope, a different key or a malformed signature **rejects the whole link**.
- `publisher` is only displayed as verified when the key is known.

### 11.3 Publisher keys

| Badge | Meaning |
| --- | --- |
| Signed by *name* (green) | The key is shipped with the app (`BUILTIN_PUBLISHERS` in `deeplink/trust.rs`, added by pull request) or the user chose to trust it earlier. The name comes from that registry, not from the link. |
| Signed by a key claiming to be *name* (blue) | Valid signature, unknown key. The user may tick "remember this key", which stores it in `~/.cc-switch/deeplink-publishers.json` (trust on first use). |
| Impersonation warning (red) | The link claims the name of a known publisher but is signed with another key. |
| Unsigned link (amber) | No signature (all v1 links). |

### 11.4 Sharing from the app

`build_deeplink_share_link(resource, app, id, includeSecrets)` returns an unsigned v2 link for a configured provider (name, homepage, endpoint, model and, only with `includeSecrets`, the API key), MCP server (env and header values blanked unless `includeSecrets`) or prompt. The link round-trips through the parser.

### 11.5 Sample signed link

Signed with a throw-away test key (`publisherKey` `RWSGmZsfeFIsiSCs/JSRfD8/Lfp1OFnCiEfSB6W7e0kULAVKwBncgFFT`, key id `892C52781F9B9986`); the envelope and signature are the fixtures in `src-tauri/src/deeplink/testdata/`. It contains a provider, an MCP server and a prompt, and shows as "signed by a key claiming to be Example Vendor":

```
ccswitch://v2/import?p=eyJ2IjoyLCJwdWJsaXNoZXIiOiJFeGFtcGxlIFZlbmRvciIsInB1Ymxpc2hlcktleSI6IlJXU0dtWnNmZUZJc2lTQ3MvSlNSZkQ4L0xmcDFPRm5DaUVmU0I2VzdlMGtVTEFWS3dCbmNnRkZUIiwiaXRlbXMiOlt7InJlc291cmNlIjoicHJvdmlkZXIiLCJhcHAiOiJjbGF1ZGUiLCJuYW1lIjoiRXhhbXBsZSBSZWxheSIsImVuZHBvaW50IjoiaHR0cHM6Ly9hcGkuZXhhbXBsZS5jb20iLCJob21lcGFnZSI6Imh0dHBzOi8vZXhhbXBsZS5jb20iLCJhcGlLZXkiOiJzay1leGFtcGxlLXBsYWNlaG9sZGVyIn0seyJyZXNvdXJjZSI6Im1jcCIsImFwcHMiOiJjbGF1ZGUsY29kZXgiLCJjb25maWciOnsibWNwU2VydmVycyI6eyJleGFtcGxlLWRvY3MiOnsiY29tbWFuZCI6Im5weCIsImFyZ3MiOlsiLXkiLCJAZXhhbXBsZS9kb2NzLW1jcCJdfX19fSx7InJlc291cmNlIjoicHJvbXB0IiwiYXBwIjoiY2xhdWRlIiwibmFtZSI6IkV4YW1wbGUgR3VpZGVsaW5lcyIsImNvbnRlbnQiOiJJeUJGZUdGdGNHeGxJR2QxYVdSbGJHbHVaWE1LTFNCQ1pTQmpiMjVqYVhObExnbz0ifV19&sig=dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUlVTR21ac2ZlRklzaVhCZnN3b3RDK0tFMnNVVXpvMzBhaVAwSFh2bm1lc3JITzJIL2hYQ2NNNEJNbGNNN1huRFBlc0RxQ3A5RFdGVHhqb2FkMjRIT0o4TnlhQWttaVM5RGdRPQp0cnVzdGVkIGNvbW1lbnQ6IGNjc3dpdGNoIGRlZXAgbGluayB0ZXN0IGZpeHR1cmUKNnlXMmhPaVlsWnRsdzFzdmtZRHhGRVpHR3I3bkkxU0ljOFQxNGRHWVVWK1YxTGl1ajkzQktLTGpqNmJvMjdlejhzWHo5ZFYxcW5GU3c0eWY1YkFqQUE9PQo
```
