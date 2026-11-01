# Privacy and Data Flows

CC Switch is a local desktop app. It has **no telemetry, analytics or crash reporting**, no CC Switch account, and no CC Switch server that receives your data. Everything it stores stays on your machine unless you turn on cloud sync.

This page lists every kind of network request the app makes, what is sent, and when, so you can decide what to enable. It describes v3.17.0 and was written from the source code; if you find a request that is not listed here, please report it as described in [SECURITY.md](SECURITY.md).

## What is stored locally

`~/.cc-switch/` below means the CC Switch data directory, which you can move in Settings (`settings.json` always stays in `~/.cc-switch/`).

| Location | Contents |
| --- | --- |
| `~/.cc-switch/cc-switch.db` (SQLite) | Providers (including **API keys**), MCP servers (including any keys in their `env`/`args`), prompts, skills, project profiles, proxy settings, usage logs (model, token counts, cost, latency, status, session id; **not** prompt or response text) |
| `~/.cc-switch/settings.json` | Device settings, including **WebDAV / S3 credentials** if cloud sync is configured |
| `~/.cc-switch/backups/` | Copies of the database (so they contain the same keys) |
| `~/.cc-switch/logs/` | App log. At the default `info` level, deep links are logged without their values; `debug` level logs more detail and can include secrets |
| `~/.cc-switch/copilot_auth.json`, `~/.cc-switch/codex_oauth_auth.json` | OAuth tokens, if you sign in to GitHub Copilot or ChatGPT inside CC Switch |
| Each tool's own config (`~/.claude/settings.json`, `~/.codex/auth.json`, ...) | The provider you switch to, including its API key, written in the tool's own format |

None of this is encrypted by CC Switch. It relies on your operating system's file permissions and disk encryption. Treat these files, database exports and backups as secrets.

## Network requests

### Made automatically

| When | Destination | What is sent |
| --- | --- | --- |
| About one second after start-up, and when you click "Check for updates" | `github.com/farion1231/cc-switch/releases/latest/download/latest.json` (Tauri updater), then the release asset if you choose to update | A standard HTTPS request (your IP address and the updater's user agent). Updates are verified against the public key in `tauri.conf.json`. |
| When you open Settings → About, or refresh tool versions | `registry.npmjs.org`, `api.github.com`, `pypi.org` | Package names only (`@anthropic-ai/claude-code`, `@openai/codex`, `@google/gemini-cli`, `@xai-official/grok`, `opencode-ai`, `openclaw`, `hermes-agent`) to look up the latest versions. |

There is no other background traffic unless you enable one of the features below.

### When you use a provider

| Feature | Destination | What is sent |
| --- | --- | --- |
| Switching providers | Nothing | CC Switch only writes config files; the CLI tools then talk to the provider themselves. |
| Local proxy / routing / failover (off by default) | The provider's endpoint | Your **full requests, including prompts, code and tool output**, plus the provider's API key, exactly as the CLI tool would send them. The proxy listens on `127.0.0.1:15721` by default; if you change the listen address to `0.0.0.0` or a LAN address, other machines on that network can use it. |
| Speed test, stream health check, "fetch models" | The provider's endpoint | The API key and a short test request or a model-list request. |
| Usage / balance queries and usage scripts (off until you configure them) | The endpoint configured for the provider or in the script | The API key, access token or user id you entered. A custom usage script is JavaScript you (or a deep link you accepted) provided; it can call any URL. |
| Official subscription quota (off until you enable it on an official provider) | `api.anthropic.com/api/oauth/usage`, `chatgpt.com/backend-api/wham/usage`, `cloudcode-pa.googleapis.com`, `oauth2.googleapis.com` (Gemini token refresh) | The OAuth token that **Claude Code, Codex or Gemini CLI already stored on your machine**. To get it, CC Switch reads the CLI's credential file (for example `~/.claude/.credentials.json`) or, on macOS, the CLI's Keychain item (`Claude Code-credentials`, `Codex Auth`, and Gemini's entry) using the `security` tool. The token is used only for that quota request. |
| GitHub Copilot sign-in | `github.com` (device flow), `api.github.com`, `api.githubcopilot.com` (or your GitHub Enterprise domain) | The OAuth device flow and, afterwards, proxied requests with the Copilot token. |
| ChatGPT (Codex OAuth) sign-in | `auth.openai.com`, `chatgpt.com` | The OAuth flow and, afterwards, proxied requests and model-list requests with that token. |

A provider sees everything you send through it. Third-party relays, including the sponsored presets listed in the README, receive your prompts and code in the same way the official APIs do. Only use providers you trust.

### Other features you trigger

| Feature | Destination | What is sent |
| --- | --- | --- |
| Skills: browse and install | `github.com` / `codeload` (repository ZIP downloads), `skills.sh/api/search` | Repository owner/name/branch; your search terms. |
| Model pricing import | `models.dev/api.json` | A plain download; nothing about you. |
| Cloud sync (WebDAV or S3, off by default) | The server you configure | Your username/password or access keys, and a sync archive that contains the database (**including API keys**) plus a manifest with this device's **hostname**. |
| Global outbound proxy test | `httpbin.org`, `www.google.com`, `api.anthropic.com` through the proxy you entered | A connectivity probe. |
| Install / update CLI tools from Settings | `claude.ai/install.sh`, `opencode.ai/install`, `raw.githubusercontent.com` (Hermes installer), or your package manager | The tool's own installer runs in a terminal you can see. |
| Hermes web UI | `127.0.0.1:9119` | A local status probe only. |
| Deep-link import | Nothing | Links are parsed locally and need your confirmation. `configUrl` (remote config) is not implemented. See [docs/deeplink-spec.md](docs/deeplink-spec.md). |

## Sponsored and affiliate links

Some built-in provider presets and README links belong to sponsors. Several of their sign-up URLs contain referral parameters (for example `aff=`, `ref=`, `invitecode=`), which can earn the sponsor's referral programme credit for the project. Opening such a link is a normal visit to that website; CC Switch itself sends nothing to sponsors. How sponsors are listed is described in the proposed [sponsor policy](docs/sponsor-policy.md).

## Turning things off

- **Update check**: there is currently no setting to disable the start-up check; block `github.com` for the app at the firewall if you need to.
- **Local proxy, failover, usage queries, subscription quota, cloud sync**: all are off until you enable them, and can be turned off in the same place.
- **Logs**: delete `~/.cc-switch/logs/` at any time; avoid sharing debug-level logs.
- **Everything**: uninstalling the app leaves `~/.cc-switch/` in place; delete it to remove all stored data and keys.
