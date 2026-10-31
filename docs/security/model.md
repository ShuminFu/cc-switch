# Security Model

This page states, in one place, what CC Switch protects and what it does not (DOCS-08). Each
statement points at the code that implements it, so it can be checked and kept true. Detailed
pages: [PRIVACY.md](../../PRIVACY.md) (every network request), [Secrets at rest and in
sync](../secrets-vault.md) (keychain vault, sync encryption), [Deep link
specification](../deeplink-spec.md) (import protocol), [Key management](key-management.md)
(release signing keys). Report problems as described in [SECURITY.md](../../SECURITY.md).

## Scope and assumptions

CC Switch is a local desktop app running as your user. It edits the configuration files of the
AI tools you use and can run a local HTTP proxy in front of them. It has no account, no
server and no telemetry.

**In scope:** other local accounts, other devices on your network, web pages open in your
browser, links you click, cloud-sync storage operators, tampered update downloads.

**Out of scope:** malware running as your user (it can read the same files and keychain
items CC Switch can), and anyone with physical access to an unlocked machine.

## Data at rest

| Data | Where | Protection | Code |
| --- | --- | --- | --- |
| Providers, MCP servers, prompts, skills, projects, usage logs (token counts and costs, never prompt text) | `~/.cc-switch/cc-switch.db` | Owner-only file permissions on Unix (`0700` dir, `0600` db). API keys are plaintext in the database unless the **keychain vault** is turned on (Settings → Advanced), which stores them in the OS keychain and leaves placeholders in the database | `config::ensure_private_dir`, `secrets/provider_vault.rs` |
| WebDAV password, S3 secret key, sync passphrase | OS keychain (falls back to `settings.json` with a warning when no keychain exists) | Keychain access control | `secrets/sync_credentials.rs` |
| Database backups | `~/.cc-switch/backups/` | Same permissions as the database; same content, so treat them as secrets | `database/backup.rs` |
| Live tool configs (`~/.claude/settings.json`, `~/.codex/auth.json`, ...) | Each tool's own directory | Written in the tool's own format, **plaintext by design** (the tool must read the key). A copy is kept before each overwrite | `adapters/live_file.rs` |
| Logs | `~/.cc-switch/logs/`, `~/.cc-switch/crash.log` | At the default `info` level, deep-link query values and secret-bearing request headers are redacted. `debug`/`trace` levels may log more; do not attach debug logs publicly without checking them | `redact_url_for_log` in `lib.rs`, header redaction in `proxy/http_client.rs` |

Windows keeps the profile's default ACL (user, `SYSTEM`, Administrators); no extra DACL is set.

## The local proxy

| Property | Behaviour | Code |
| --- | --- | --- |
| Listen address | `127.0.0.1:15721` by default. Binding to `0.0.0.0` or a LAN address shows a warning in the proxy panel | `proxy/types.rs` |
| Who can call it | Requests from the local machine with a `localhost`/IP `Host` header are accepted. Any other request (another LAN host, a DNS-rebinding hostname) must send the per-install access token, compared in constant time | `proxy/server/access_guard.rs` |
| Browsers | Any request whose `Origin` is a non-local web origin (or `null`) is refused with `403`, so a web page cannot drive the proxy | `proxy/server/access_guard.rs` |
| Body size | Requests are capped at 64 MiB | `MAX_REQUEST_BODY_BYTES` |
| Keys in live files during takeover | While an app is taken over, its live config holds the placeholder `PROXY_MANAGED` instead of the real key; the proxy injects the real key upstream. The original file is backed up in the database and restored when takeover stops | `services/proxy.rs` (`PROXY_TOKEN_PLACEHOLDER`, `backup_live_config_strict`) |

The access token is not a substitute for TLS: the proxy speaks plain HTTP. Do not expose it beyond
a trusted network.

## Deep links (`ccswitch://`)

- Every import shows a confirmation dialog with the decoded content; nothing is written before
  you confirm. Switching to the imported provider, enabling MCP servers and enabling usage
  scripts each need an explicit checkbox.
- Imported MCP servers show the full command line before you accept.
- v2 links can be signed (minisign); the dialog shows whether a link is signed by a known
  publisher.
- Deep-link URLs are logged with their query values redacted.

Details: [deep link specification, §8](../deeplink-spec.md#8-user-confirmation-and-security-model).

## Cloud sync and backups

- WebDAV and S3 sync upload the database snapshot (which contains API keys unless the keychain
  vault is on). Turn on **end-to-end sync encryption** (Argon2id + XChaCha20-Poly1305) so the
  storage operator cannot read it; see [Secrets at rest and in sync](../secrets-vault.md).
- Device-local state (proxy listen address, takeover state, outbound proxy) is never synced.
- Plain `http://` endpoints to public hosts are flagged; use `https://`.
- Downloads and restores take a safety backup first and verify the incoming database before it
  replaces the live one.

## Updates and downloads

- The in-app updater only installs packages whose minisign signature matches the public key
  compiled into the app (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`). The release
  workflow verifies every signature before it publishes `latest.json`.
- macOS builds are signed with a Developer ID certificate and notarized by Apple.
- Windows builds are Authenticode-signed only when the maintainers' certificate is configured
  (not yet the case for every release); see [README](../../README.md#download--installation).
- Every release carries `SHA256SUMS`, a CycloneDX SBOM and GitHub build-provenance
  attestations. See [Verifying a download](../../SECURITY.md#verifying-a-download).
- How the signing keys are held and rotated: [Key management](key-management.md).

## Known limitations

- API keys are plaintext in the database unless you enable the keychain vault, and always
  plaintext in the tools' own config files.
- The proxy has no TLS. LAN exposure relies on the access token and your network.
- Same-user malware is not defended against.
- The Flatpak build (community) is sandboxed differently and cannot run host tools yet.
