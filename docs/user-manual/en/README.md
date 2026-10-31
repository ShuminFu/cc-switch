# CC Switch User Manual

> All-in-One Assistant for Claude Code / Claude Desktop / Codex / Gemini CLI / Grok Build / OpenCode / OpenClaw / Hermes

## Table of Contents

```
CC Switch User Manual
│
├── 1. Getting Started
│   ├── 1.1 Introduction
│   ├── 1.2 Installation Guide
│   ├── 1.3 Interface Overview
│   ├── 1.4 Quick Start
│   ├── 1.5 Personalization
│   ├── 1.6 Projects
│   └── 1.7 What Works for Each App
│
├── 2. Provider Management
│   ├── 2.1 Add Provider
│   ├── 2.2 Switch Provider
│   ├── 2.3 Edit Provider
│   ├── 2.4 Sort & Duplicate
│   ├── 2.5 Usage Query
│   ├── 2.6 Claude Desktop
│   └── 2.7 Grok Build
│
├── 3. Extensions
│   ├── 3.1 MCP Server Management
│   ├── 3.2 Prompts Management
│   ├── 3.3 Skills Management
│   ├── 3.4 Session Manager
│   └── 3.5 Workspace & Memory
│
├── 4. Proxy & High Availability
│   ├── 4.1 Proxy Service
│   ├── 4.2 App Takeover
│   ├── 4.3 Failover
│   ├── 4.4 Usage Statistics
│   ├── 4.5 Model Test
│   └── 4.6 Proxy Semantics
│
└── 5. FAQ
    ├── 5.1 Configuration Files
    ├── 5.2 FAQ
    ├── 5.3 Deep Link Protocol
    ├── 5.4 Environment Variable Conflicts
    ├── 5.5 Command Line (ccs)
    └── 5.6 Troubleshooting
```

## File List

### 1. Getting Started

| File | Description |
|------|-------------|
| [1.1-introduction.md](./1-getting-started/1.1-introduction.md) | Introduction, core features, supported platforms |
| [1.2-installation.md](./1-getting-started/1.2-installation.md) | Windows/macOS/Linux installation guide |
| [1.3-interface.md](./1-getting-started/1.3-interface.md) | Interface layout, navigation bar, provider cards |
| [1.4-quickstart.md](./1-getting-started/1.4-quickstart.md) | 5-minute quick start tutorial |
| [1.5-settings.md](./1-getting-started/1.5-settings.md) | Language, theme, directories, backups, WebDAV / S3 cloud sync |
| [1.6-projects.md](./1-getting-started/1.6-projects.md) | Project profiles: save and switch provider / MCP / Skills / prompt snapshots |
| [1.7-capabilities.md](./1-getting-started/1.7-capabilities.md) | Which features (MCP, prompts, Skills, takeover, projects, deep links) apply to which app |

### 2. Provider Management

| File | Description |
|------|-------------|
| [2.1-add.md](./2-providers/2.1-add.md) | Using presets, custom configuration, universal providers |
| [2.2-switch.md](./2-providers/2.2-switch.md) | Main UI switching, tray switching, activation methods |
| [2.3-edit.md](./2-providers/2.3-edit.md) | Edit configuration, modify API Key, backfill mechanism |
| [2.4-sort-duplicate.md](./2-providers/2.4-sort-duplicate.md) | Drag-to-reorder, duplicate provider, delete |
| [2.5-usage-query.md](./2-providers/2.5-usage-query.md) | Usage query, remaining balance, multi-plan display |
| [2.6-claude-desktop.md](./2-providers/2.6-claude-desktop.md) | Claude Desktop third-party providers, direct mode, and model mapping |
| [2.7-grok-build.md](./2-providers/2.7-grok-build.md) | Grok Build providers, `config.toml`, proxy takeover |

### 3. Extensions

| File | Description |
|------|-------------|
| [3.1-mcp.md](./3-extensions/3.1-mcp.md) | MCP protocol, add servers, app binding |
| [3.2-prompts.md](./3-extensions/3.2-prompts.md) | Create presets, activate/switch, smart backfill |
| [3.3-skills.md](./3-extensions/3.3-skills.md) | Discover skills, install/uninstall, repository management |
| [3.4-sessions.md](./3-extensions/3.4-sessions.md) | Session Manager: browse, search, resume, delete sessions |
| [3.5-workspace.md](./3-extensions/3.5-workspace.md) | Workspace files and daily memory (OpenClaw) |

### 4. Proxy & High Availability

| File | Description |
|------|-------------|
| [4.1-service.md](./4-proxy/4.1-service.md) | Start proxy, configuration, LAN access token, running status |
| [4.2-routing.md](./4-proxy/4.2-routing.md) | App routing, configuration changes, status indicators |
| [4.3-failover.md](./4-proxy/4.3-failover.md) | Failover queue, circuit breaker, health status |
| [4.4-usage.md](./4-proxy/4.4-usage.md) | Usage statistics, trend charts, pricing configuration |
| [4.5-model-test.md](./4-proxy/4.5-model-test.md) | Model test, health check, latency testing |
| [4.6-semantics.md](./4-proxy/4.6-semantics.md) | Which errors fail over, timeouts, circuit breaker rules, what takeover changes on disk |

### 5. FAQ

| File | Description |
|------|-------------|
| [5.1-config-files.md](./5-faq/5.1-config-files.md) | CC Switch storage, CLI configuration file formats |
| [5.2-questions.md](./5-faq/5.2-questions.md) | Frequently asked questions |
| [5.3-deeplink.md](./5-faq/5.3-deeplink.md) | Deep link protocol, generation and usage |
| [5.4-env-conflict.md](./5-faq/5.4-env-conflict.md) | Environment variable conflict detection and resolution |
| [5.5-command-line.md](./5-faq/5.5-command-line.md) | `ccs` command-line companion and headless proxy mode |
| [5.6-troubleshooting.md](./5-faq/5.6-troubleshooting.md) | Per-OS fixes, log and crash-log locations, filing a bug report |

## Quick Links

- **New users**: Start with [1.1 Introduction](./1-getting-started/1.1-introduction.md)
- **Installation issues**: See [1.2 Installation Guide](./1-getting-started/1.2-installation.md)
- **Configure providers**: See [2.1 Add Provider](./2-providers/2.1-add.md)
- **Use Claude Desktop**: See [2.6 Claude Desktop](./2-providers/2.6-claude-desktop.md)
- **Use Grok Build**: See [2.7 Grok Build](./2-providers/2.7-grok-build.md)
- **Switch whole setups at once**: See [1.6 Projects](./1-getting-started/1.6-projects.md)
- **Using proxy**: See [4.1 Proxy Service](./4-proxy/4.1-service.md)
- **Having trouble**: See [5.6 Troubleshooting](./5-faq/5.6-troubleshooting.md) and [5.2 FAQ](./5-faq/5.2-questions.md)
- **Step-by-step guides**: See [docs/guides](../../guides/README.md)

## Version Information

- Documentation version: v3.17.0
- Last updated: 2026-10-30
- Applicable to CC Switch v3.17.0+

### Changes After v3.17.0 (Next Release)

- **Proxy access token for LAN access**: when the proxy listens on a non-loopback address, other devices must send a per-install access token; the panel shows a warning and the token. See [4.1 Proxy Service](./4-proxy/4.1-service.md)
- **Deep link opt-in**: a deep link no longer switches providers, enables MCP servers or enables usage scripts unless you tick the matching checkbox; MCP imports show the full command. See [5.3 Deep Link Protocol](./5-faq/5.3-deeplink.md)
- **Sync conflict detection**: WebDAV / S3 uploads and downloads refuse to overwrite changes made on another device or locally since the last sync; plain `http://` endpoints are flagged. See [1.5 Personalization](./1-getting-started/1.5-settings.md)
- **Backup kinds**: database backups are named by kind (scheduled, manual, safety, pre-migration) and rotated per kind; manual backups are never auto-deleted; restores are validated first. See [1.5 Personalization](./1-getting-started/1.5-settings.md)
- **Live file safety**: live config files are backed up before they are overwritten, and an unparsable file stops the switch instead of being replaced. See [5.1 Configuration Files](./5-faq/5.1-config-files.md)

### v3.17.0 Highlights

- **Project profiles**: save the provider, MCP, Skills and prompt state of Claude Code, Codex or Claude Desktop as a named project and switch it from the header or the tray. See [1.6 Projects](./1-getting-started/1.6-projects.md)
- **Grok Build**: first-class support for xAI's Grok Build CLI, including providers, MCP, Skills, prompts, sessions and proxy takeover. See [2.7 Grok Build](./2-providers/2.7-grok-build.md)
- **Codex official sessions through the proxy**: official ChatGPT-subscription Codex sessions can be routed through the local proxy takeover; Codex providers can also target a native Anthropic Messages upstream. See [2.1 Add Provider](./2-providers/2.1-add.md)

### v3.16.0 Highlights

- **Codex Chat Completions routing**: route Chat-only providers such as DeepSeek, Kimi, GLM, and MiniMax through Codex. See [2.1 Add Provider](./2-providers/2.1-add.md)
- **Managed CLI tool lifecycle**: install, update, update all, and diagnose Claude / Codex / Gemini / OpenCode / OpenClaw / Hermes from Settings / About. See [1.5 Personalization](./1-getting-started/1.5-settings.md)
- **Provider and model refresh**: new partner presets, refreshed default models and pricing, Claude Opus 4.8 defaults, and GPT 5.5 defaults where applicable
- **Routing support badges**: Claude Code / Codex provider cards indicate whether a provider can be served through Local Routing
- **Codex OAuth live model discovery**: ChatGPT Codex providers fetch available models from the ChatGPT backend on demand
- **Filter-driven Usage Hero**: shows cache-normalized real total tokens and cache hit rate, updating with date / provider / model filters — see [4.4 Usage Statistics](./4-proxy/4.4-usage.md)
- **Lightweight Mode**: Destroys the main window when minimizing to tray — near-zero idle footprint. See [1.5 Personalization](./1-getting-started/1.5-settings.md)
- **Quota & Balance Display**: Official subscriptions (Claude/Codex/Gemini/Copilot/Codex OAuth) auto-display quotas; Token Plan and third-party balances use built-in templates with one-click enable — see [2.5 Usage Query](./2-providers/2.5-usage-query.md)
- **Codex OAuth Reverse Proxy**: Reuse your ChatGPT account's Codex service inside Claude Code — see [2.1 Add Provider](./2-providers/2.1-add.md)
- **Per-App Tray Submenus**: Claude / Codex / Gemini submenus show the current provider and available usage summaries — see [2.2 Switch Provider](./2-providers/2.2-switch.md)
- **Skills Discovery & Batch Updates**: SHA-256 update detection, batch updates, skills.sh public registry search — see [3.3 Skills Management](./3-extensions/3.3-skills.md)
- **Full URL Endpoint Mode**: Advanced option to treat `base_url` as the full upstream endpoint — see [2.1 Add Provider](./2-providers/2.1-add.md)
- **OpenCode / OpenClaw / Hermes Stream Check Coverage**: Stream Check covers Claude / Codex / Gemini / OpenCode / OpenClaw / Hermes — see [4.5 Model Test](./4-proxy/4.5-model-test.md)

## Contributing

Feel free to submit Issues or PRs to improve the documentation:

- [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- [GitHub Repository](https://github.com/farion1231/cc-switch)
