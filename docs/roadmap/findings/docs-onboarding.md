# Documentation & onboarding — findings (DOCS-xx)

Part of the [next-phase proposals](../README.md). 23 findings. Priorities: P2 = 15, P3 = 8.

## Area summary

The four UI locales (`src/i18n/locales/{en,zh,zh-TW,ja}.json`) are structurally healthy: 2,583 flattened keys in en/zh/ja, identical key order, 0 placeholder mismatches, 0 whitespace defects, 0 duplicate keys inside a file; the only parity gap is zh-TW with 30 missing keys (all `settings.tool*` tool-lifecycle strings, now rendered in English via `fallbackLng: "en"`) plus 2 orphan keys. Cross-referencing 2,394 literal `t()` calls (1,838 distinct keys, 14 template-literal and 39 dynamic-expression calls marked unverifiable) found 10 keys used in code but absent from every locale — each falls back to a hard-coded Chinese `defaultValue`, so English/Japanese/Traditional-Chinese users see Simplified Chinese toasts (e.g. `usage.unpriced` → "未定价") — and ~444 en keys (17%) that no code path references (the whole `console.*` namespace, 53 `omo.*`, 45 `proxy.*`, 44 `mcp.*`). Beyond JSON, the i18n boundary is porous: 802 `defaultValue:` fallbacks of which 459 are Chinese plus 195 positional `t("key", "中文")` defaults, ~247 CJK string literals outside `t()` (zod/JSON validation messages), and a Rust backend whose user-facing errors are Chinese-only (525 `Err/format!` lines, 53 `#[error("…")]` attributes, 228 commands returning `Result<_, String>`), which the frontend then matches by Chinese substring (`src/utils/errorUtils.ts:45-110`); proxy HTTP error bodies sent to Claude Code/Codex are Chinese too. First-run language detection falls back to `zh` for any non-CJK/non-English OS locale on both the frontend (`src/i18n/index.ts:11,57`) and the tray (`src-tauri/src/tray.rs:80-83,90-94`), so a German or Korean user gets a Chinese UI even though a German README exists. Process-wise, CONTRIBUTING.md still names three languages and the wrong paths, `.github/labeler.yml` watches a non-existent `src/locales/**`, `pnpm lint` is referenced but not defined, and CI (`.github/workflows/ci.yml`) has no locale-parity, dead-key or hard-coded-string check. On documentation: all four READMEs share an identical 589-line skeleton (11 FAQ items, 14 feature bullets each) but link release notes for v3.16.1 while the app is 3.17.0, the trilingual user manual (3×26 files) is stamped v3.16.0/2026-05-29 and has zero mentions of Grok Build, project profiles or S3 sync, the deep-link manual/generator cover 5/3 apps while the parser accepts 7, the README screenshots show a 3-app UI, 4 old versions lack Japanese release notes, 14 relative links in v3.7–3.8 release notes are broken, the CHANGELOG lacks a 3.6.1 entry and has a 3.5.0 date out of sequence. Onboarding is a single first-run modal whose text names only three tools, with no setup wizard, no in-app link to the manual, no in-app "what's new", and undocumented keyboard shortcuts.

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [DOCS-01](#docs-01) | The user manual is frozen at v3.16.0 and omits Grok Build, project profiles and S3 sync | docs | P2 (first pass P1) | L | high | partially-confirmed |
| [DOCS-02](#docs-02) | Release-notes/changelog hygiene: stale README link, no index, missing ja for 4 versions, 14 broken links, a missing 3.6.1 entry and a mis-dated 3.5.0 | docs | P2 (first pass P1) | S | high | partially-confirmed |
| [DOCS-04](#docs-04) | Deep-link protocol has no canonical spec; manual and generator lag the parser | docs | P2 (first pass P1) | M | high | partially-confirmed |
| [DOCS-05](#docs-05) | Proxy / failover runtime semantics are undocumented beyond the UI walkthrough; proxy guide is Chinese-only | docs | P2 | M | medium |  |
| [DOCS-06](#docs-06) | Cloud-sync (WebDAV/S3) conflict and scope semantics are not documented | docs | P2 | M | medium |  |
| [DOCS-07](#docs-07) | Backup/restore procedure and the Backup Manager are undocumented | docs | P2 | S | high |  |
| [DOCS-08](#docs-08) | No security-model document (what is stored where, in what form, and what the proxy exposes) | docs | P2 | M | medium |  |
| [DOCS-09](#docs-09) | No contributor architecture documentation beyond a collapsed README section duplicated four times | docs | P2 | M | high |  |
| [DOCS-10](#docs-10) | Guides, the session-manager PRD and the deep-link generator are not discoverable | docs | P2 | S | high |  |
| [DOCS-11](#docs-11) | README front-loads 175 lines of sponsor banners before any product description | docs | P2 | M | medium |  |
| [DOCS-14](#docs-14) | Import-from-existing-configs is silent and per-app; surface a cross-app import summary | feature | P2 | M | medium |  |
| [DOCS-15](#docs-15) | Add a health checklist / diagnostics panel with "copy diagnostics" | feature | P2 | L | medium |  |
| [DOCS-16](#docs-16) | No contextual help links from the app to the manual | feature | P2 | M | high |  |
| [DOCS-17](#docs-17) | In-app "What's new" after updates | feature | P2 | M | high |  |
| [DOCS-19](#docs-19) | Error presentation: raw backend strings in toasts, mixed languages, no "copy details" or next step | ux | P2 | M | high |  |
| [DOCS-03](#docs-03) | README screenshots show a three-app UI that predates most shipped features | docs | P3 (first pass P1) | M | high | partially-confirmed |
| [DOCS-12](#docs-12) | Cross-README consistency nits (relative `../../releases` links, naming, DE manual link) | docs | P3 | S | high |  |
| [DOCS-13](#docs-13) | First-run experience is a single text modal — add a guided setup wizard | ux | P3 (first pass P1) | L | medium | partially-confirmed |
| [DOCS-18](#docs-18) | Keyboard shortcuts exist but are undocumented and platform-unaware | ux | P3 | S | high |  |
| [DOCS-20](#docs-20) | Per-OS troubleshooting is scattered and lacks log locations | docs | P3 | S | medium |  |
| [DOCS-21](#docs-21) | Issue/PR templates do not reflect four UI locales or the German README | docs | P3 | S | high |  |
| [DOCS-22](#docs-22) | GitHub release body is Chinese-only and hand-written in the workflow | docs | P3 | S | high |  |
| [DOCS-23](#docs-23) | Package/store descriptions and in-app product copy still describe a three-tool product | docs | P3 | S | high |  |

## Findings

### DOCS-01: The user manual is frozen at v3.16.0 and omits Grok Build, project profiles and S3 sync

**Type:** docs · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `docs/user-manual/README.md:3` lists "Claude Code / Claude Desktop / Codex / Gemini CLI / OpenCode / OpenClaw / Hermes" (no Grok Build) and `:15-17` "Documentation version: v3.16.0 / Last updated: 2026-05-29"; `docs/user-manual/{en,zh,ja}/README.md:109-113` same stamp and "v3.16.0 Highlights"; `rg -il grok docs/user-manual` → 0 files in all three locales; `rg -il "project profile|named snapshot" docs/user-manual/en` → 0 while `CHANGELOG.md:8-10` makes profiles the 3.17.0 headline and `docs/release-notes/v3.17.0-en.md:9-11` points readers at the manual; `rg -il "\bS3\b" docs/user-manual/en` → 0 although `src/i18n/locales/en.json` has `settings.s3Sync.*` (presets AWS S3 / R2 / OSS / COS / OBS); `README.md:218` "[Release Notes](docs/release-notes/v3.16.1-en.md)" in all four READMEs; `package.json:3` version 3.17.0. Coverage is otherwise symmetric: 26 files per locale (en 191,616 B, zh 179,398 B, ja 248,023 B), 40 shared assets.
- **Problem:** The manual is the only deep documentation and it lags one minor release plus the Grok Build client (commit 1c0ee0c); users of the headline 3.17 feature have nothing to read; the version stamp hand-maintained in 4 places is already wrong.
- **Proposal:** Make "manual updated or explicitly N/A" a release checklist item in `.github/pull_request_template.md`; add `docs/user-manual/en/2-providers/2.7-grok-build.md` and `1-getting-started/1.6-projects.md` (+ zh/ja), extend 1.5-settings with S3 sync; derive the version stamp from `package.json` with a tiny script run in the release workflow; add a `docs` CI job that fails when `src/config/*Presets.ts` gains an app id not mentioned in the manual index.
- **Acceptance:** Manual index lists 8 tools and stamps 3.17.0; `rg -il grok docs/user-manual/en | wc -l` ≥ 2; release PR template has the docs checkbox.
- **Verification:** **partially-confirmed** — Every cited fact holds: docs/user-manual/README.md:3 lists seven tools without Grok Build and :15-17 stamps 'v3.16.0 / 2026-05-29', the same stamp sits at en/zh/ja README.md:109-111; `rg -il grok docs/user-manual` returns nothing although commit 1c0ee0c (2026-07-17) added Grok Build and src-tauri/src/app_config.rs:17 carries `grokbuild`; `rg -il 'project profile|named snapshot'` and case-sensitive `rg 'S3'` return nothing in the en manual while CHANGELOG.md:16 headlines project profiles, src/components/profiles/ exists and src-tauri/src/commands/s3_sync.rs plus en.json:565-578 ship S3 presets; README.md:218 (and ZH:219/JA:218/DE:218) still link v3.16.1 notes with package.json:3 at 3.17.0, and .github/pull_request_template.md:20-25 has no docs checkbox and no workflow in .github/workflows touches docs. The impact claim 'users of the headline 3.17 feature have nothing to read' is overstated: CHANGELOG.md:16 and docs/release-notes/v3.17.0-en.md:3 describe projects at length, and the release note's Usage Guides (v3.17.0-en.md:13-16) point at docs/guides files that exist. A one-release manual lag is a medium documentation gap, not high, so P2; the proposal is sound and unimplemented, but the CI guard should key off the AppType/VisibleApps definitions in src-tauri/src/settings.rs:27-49 rather than only src/config/*Presets.ts.

### DOCS-02: Release-notes/changelog hygiene: stale README link, no index, missing ja for 4 versions, 14 broken links, a missing 3.6.1 entry and a mis-dated 3.5.0

**Type:** docs · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `README.md:218`, `README_ZH.md:219`, `README_JA.md:218`, `README_DE.md:218` link `docs/release-notes/v3.16.1-*.md` (current 3.17.0); `ls docs/release-notes` = 68 files / 23 versions, `v3.6.0`, `v3.6.1`, `v3.7.0`, `v3.7.1` have en+zh only; no `docs/release-notes/README.md`; `linkcheck.py`: 14 broken relative links in `docs/release-notes/v3.7.0-{en,zh}.md:424-425`, `v3.7.1-{en,zh}.md:466-467`, `v3.8.0-{en,zh,ja}.md:354-355/300-301` (`../README.md`, `../CHANGELOG.md` should be `../../`); `CHANGELOG.md` has no `[3.6.1]` heading (`rg "3\.6\.1" CHANGELOG.md` → none) while `docs/release-notes/v3.6.1-*.md` exist; `CHANGELOG.md:2007` "## [3.5.0] - 2025-01-15" sits between 3.6.0 (2025-11-07, `:1894`) and 3.4.0 (2025-10-01, `:2060`); section headings use three vocabularies (28× "Fixed"/27× "Added"/22× "Changed" vs 5× "🐛 Bug Fixes"/5× "✨ New Features" vs "Feature Release"/"Patch Release"/"Beta Release"), 12 lines contain Chinese.
- **Problem:** Readers following the README land on last-but-one notes; the changelog cannot be parsed by tooling (keep-a-changelog claims at `CHANGELOG.md:5`); the Japanese audience lacks four early release notes.
- **Proposal:** Point the README link at a stable `docs/release-notes/README.md` index (auto-generated table: version × en/zh/ja); fix the 14 links; add the 3.6.1 entry and correct the 3.5.0 date (likely 2025-10-15); normalise headings to Added/Changed/Fixed/Removed/Docs; translate or mark the four missing ja notes; run `linkcheck.py` in CI for `docs/**` and `README*.md`.
- **Acceptance:** `linkcheck.py` reports `broken_local=0`; `rg -c "^### " CHANGELOG.md` uses ≤ 6 distinct headings; README links resolve to the current version.
- **Verification:** **partially-confirmed** — Confirmed: README.md:218/README_ZH.md:219/README_JA.md:218/README_DE.md:218 link docs/release-notes/v3.16.1-*.md; docs/release-notes holds 68 files with v3.6.0, v3.6.1, v3.7.0 and v3.7.1 present only as en+zh and no README.md index; `rg '\]\(\.\./(README[_A-Z]*|CHANGELOG)\.md\)' docs/release-notes` yields exactly 14 hits (v3.7.0-en/zh:424-425, v3.7.1-en/zh:466-467, v3.8.0-en/zh:354-355, v3.8.0-ja:300-301) and docs/README.md / docs/CHANGELOG.md do not exist; `rg '3\.6\.1' CHANGELOG.md` is empty while docs/release-notes/v3.6.1-en.md:9 is dated 2025-11-10; CHANGELOG.md:2007 '## [3.5.0] - 2025-01-15' sits between :1894 (3.6.0, 2025-11-07) and :2060 (3.4.0, 2025-10-01); the `### ` heading tally is exactly 28 Fixed / 27 Added / 22 Changed / 5 each of the emoji variants / 4 Feature Release / 3 Patch / 3 Beta (47 distinct headings) and 12 lines contain Han characters. Details off: there are 24 versions, not 23; the true 3.5.0 date cannot be established (shallow 50-commit clone, no tags) so '2025-10-15' is a guess; `linkcheck.py` is the reviewer's own scratch script (scratchpad/i18n-tools/), not a repo tool, so the proposal should name a real checker; and nothing in .github/workflows, package.json or scripts/ parses CHANGELOG.md, so the 'tooling cannot parse it' harm is hypothetical. All of this is hygiene with S effort and no functional impact, so P2 rather than P1.

### DOCS-04: Deep-link protocol has no canonical spec; manual and generator lag the parser

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/deeplink/parser.rs:59-66` dispatches `provider|prompt|mcp|skill`; `:84,193` accept apps `claude|codex|gemini|grokbuild|opencode|openclaw|hermes`, `:268-272` also `grok`; `docs/user-manual/en/5-faq/5.3-deeplink.md:42` documents `claude / codex / gemini / opencode / openclaw` only (zh/ja identical); `deplink.html:693-695,910-912` offers `claude|codex|gemini` only, is `lang="zh-CN"` with a Chinese title (`:2,7`), lives at the repo root, is linked from no README and only from the manual via `farion1231.github.io/cc-switch/deplink.html` (`5.3-deeplink.md:16`); the parser's parameter list (`apiKey`, `haikuModel`, `configFormat`, `usage*`, `repo`, `directory`, `branch` …) is described only in prose tables, with no mention of URL-encoding rules, max length, or version negotiation beyond "V1".
- **Problem:** Third parties (provider vendors publish "import into CC Switch" links) cannot rely on a spec; the generator cannot produce links for 4 of the 7 apps; non-Chinese vendors cannot use the generator at all.
- **Proposal:** Write `docs/spec/deeplink-v1.md` (grammar, parameter table per resource generated from a Rust table that also drives `parser.rs` tests, encoding, error behaviour, security model) in en/zh/ja; update the generator to read the same table, add en/ja UI, move it to `docs/tools/` or the website; link both from README "Documentation".
- **Acceptance:** Every app id accepted by `parser.rs:84` appears in the spec and the generator's `<select>`; a Rust test fails when a new resource/app is added without updating the spec table.
- **Verification:** **partially-confirmed** — Confirmed: src-tauri/src/deeplink/parser.rs:59-63 dispatches provider/prompt/mcp/skill, :84 and :193 accept `claude|codex|gemini|grokbuild|opencode|openclaw|hermes`, and :265-272 additionally accepts `grok` for the MCP apps list; docs/user-manual/{en,zh,ja}/5-faq/5.3-deeplink.md:42 lists only `claude / codex / gemini / opencode / openclaw` (missing grokbuild and hermes); deplink.html:2 is `lang="zh-CN"`, :7 has a Chinese title, its only app selectors at :693-695 and :910-912 offer claude/codex/gemini, it sits at the repo root, is referenced by no README and only by 5.3-deeplink.md:16, and docs/spec does not exist. Overstated: 'no canonical spec' ignores that 5.3-deeplink.md:27-50 already carries a grammar line and per-resource parameter tables (incomplete, not absent), and the parser is not untested — src-tauri/src/deeplink/tests.rs has 31 tests including grokbuild cases at :91, :537, :777 and :805, plus 2 in src-tauri/tests/deeplink_import.rs — so the proposed table-driven drift test is additive rather than filling a void. Manual and generator lagging the parser by 2 and 4 apps is a real but medium-impact doc gap for third-party link authors, so P2.

### DOCS-05: Proxy / failover runtime semantics are undocumented beyond the UI walkthrough; proxy guide is Chinese-only

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `docs/user-manual/en/4-proxy/4.3-failover.md` headings (Overview, Prerequisites, Configure Queue, Enable, Failover Flow, Circuit Breaker Configuration/States, Health Status, Logs, Best Practices, FAQ) and `4.1-service.md` (Start/Config/How It Works/API Format Conversion/Stop/Log) describe clicks, not contracts: no table of which HTTP statuses/timeouts count as failures, retry order vs. queue order, what "takeover" rewrites in live config files and how it is reverted (`4.1-service.md:177` "Post-stop Processing" only), Claude Desktop "local route" vs. takeover exclusivity (`src/components/proxy/ClaudeDesktopRouteToggle.tsx:44-47` shows a user-facing conflict rule), the 3.17 "fail closed" bridge behaviour (`CHANGELOG.md:10`); `docs/guides/proxy-guide-zh.md` exists only in zh (every other guide has en/zh/ja: 6 guides × 3); proxy settings strings for timeouts/retries exist in `en.json` (`proxy.settings.fields.*`, `proxyConfig.*`) but most are in the dead-key list (I18N-07), suggesting the UI moved and docs were not revisited.
- **Problem:** Users cannot predict failover behaviour (common FAQ topics "Failover Not Triggering"/"Triggering Too Frequently" in `5.2-questions.md:118-137` are symptoms of this), and English/Japanese readers have no proxy guide.
- **Proposal:** Add `docs/user-manual/*/4-proxy/4.6-semantics.md` with a decision table (status code → counts as failure?, retry?, open circuit?), timeout definitions (first-byte, idle, non-streaming), queue/takeover state machine diagram, and config-file side effects per app; translate `proxy-guide` to en/ja or fold it into the manual; link the proxy error codes from I18N-12.
- **Acceptance:** The manual answers "which responses trigger failover" with a table; `docs/guides` has `proxy-guide-{en,ja}.md` or the zh guide is removed in favour of the manual.

### DOCS-06: Cloud-sync (WebDAV/S3) conflict and scope semantics are not documented

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `rg -il webdav docs/user-manual/en` → 1 file (`1-getting-started/1.5-settings.md`), `rg -il "\bS3\b" docs/user-manual/en` → 0; the UI has conflict prompts (`en.json` keys `settings.webdavSync.confirmDownload.*`/`confirmUpload.*` "Uploaded by"/"Uploaded at" device name, `settings.s3Sync.mutualExclusionMessageReverse`, `settings.syncLiveFailed`) and five S3 presets (`settings.s3Sync.presets.{awsS3,r2,oss,cos,obs}`); `README.md:248` advertises "Cloud sync — Custom config directory (Dropbox, OneDrive, iCloud, NAS) and WebDAV server sync" (no S3); `README.md:297-305` FAQ lists data locations but not what is synced vs device-local.
- **Problem:** Users running two machines cannot learn what wins on conflict, whether `settings.json` (device-level) is excluded, whether API keys travel in clear text to the bucket, or why WebDAV and S3 are mutually exclusive.
- **Proposal:** Add `1.6-cloud-sync.md` (what is synced: `cc-switch.db` only?; conflict rule: last-upload-wins with device-name prompt; exclusivity; encryption/at-rest guidance; recovery via backups) in en/zh/ja; update README feature bullet to include S3-compatible storage.
- **Acceptance:** Manual has a sync page with a conflict table; README bullet mentions S3; the two confirm-dialog strings link (or refer) to it.

### DOCS-07: Backup/restore procedure and the Backup Manager are undocumented

**Type:** docs · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `docs/user-manual/en/5-faq/5.1-config-files.md:61-67` "Automatic Backups" and `:340-360` "Backup Recommendations" exist, but `rg -i restore 5.1-config-files.md` → 0 matches; `README.md:297-305` lists `~/.cc-switch/backups/` (keeps 10) and `skill-backups/` (keeps 20) without a restore path; the app has a Backup Manager UI (`en.json` keys `settings.backupManager.description`, `.deleteConfirmTitle`, `.intervalDisabled`, `src/components/settings/BackupListSection.tsx`) and skills restore (`skills.restoreFromBackup.*`), none mentioned in the manual; `src/main.tsx:49-56` tells users to "restore from a backup file (e.g., config.json.bak)" on fatal load.
- **Problem:** The only moment a user needs restore instructions (corrupted DB on startup) is exactly when the UI is unavailable, and there is no page to point to.
- **Proposal:** Add "Backup & Restore" to 5.1 (automatic backup cadence/retention, Backup Manager UI, manual restore of `cc-switch.db` with the app closed, skill backups, what the fatal-load dialog means), and link that section from the fatal dialog text (`errors.configLoadFailedMessage`).
- **Acceptance:** Manual contains a step-by-step restore; fatal dialog message references the doc URL.

### DOCS-08: No security-model document (what is stored where, in what form, and what the proxy exposes)

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `SECURITY.md` covers only vulnerability reporting and response times; `rg -il "keyring|keychain|encrypt" src-tauri/src` hits only TLS/streaming code (no secret-store usage), so API keys live in `~/.cc-switch/cc-switch.db` SQLite as stored; the manual has scattered security notes (`5.3-deeplink.md:197-221` "Security Considerations", `5.2-questions.md:207` "risks of enabling the Codex OAuth reverse proxy", `4.1-service.md:51` "Listen Address Options"); proxy and takeover rewrite live CLI config files (`4.1-service.md:130`); `src-tauri/src/lib.rs:228` writes `crash.log` to the config dir.
- **Problem:** Security-conscious users and enterprise adopters cannot find a single statement of the threat model: plaintext secrets at rest, local proxy bind address and auth (none?), what deep links can do, what cloud sync uploads, what logs may contain keys.
- **Proposal:** Add a "Security model" section to `SECURITY.md` (or `docs/security.md` en/zh/ja): data at rest, proxy listener and recommended bind, deep-link confirmation flow, sync/backup contents, log redaction policy, update channel signing (`README.md:278,374` notes macOS notarization); list known limitations (no keychain) and roadmap.
- **Acceptance:** SECURITY.md links to the model; FAQ "Where is my data stored?" links to it; each statement cites the code path.

### DOCS-09: No contributor architecture documentation beyond a collapsed README section duplicated four times

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `README.md:395-570` "Architecture Overview" (ASCII diagram, Tech Stack) is repeated in `README_ZH/JA/DE.md`; `find . -iname "*architecture*" -o -iname AGENTS.md -o -iname CLAUDE.md` → none; `CONTRIBUTING.md:103-115` sets an "AI-Assisted Contributions" policy but offers no module map for humans or agents; `.github/CODEOWNERS` routes everything to one owner; backend has 10+ subsystems (`src-tauri/src/{proxy,services,database,deeplink,session_manager,mcp,settings,tray}`), DB schema v13 (`CHANGELOG.md:10`), a `test-hooks` feature (`README.md:495`).
- **Problem:** Review load concentrates on one maintainer; drive-by (often AI-generated) PRs lack a map of invariants (atomic writes, live-config projection, takeover/backfill rules), which the policy text itself complains about.
- **Proposal:** Add `docs/dev/architecture.md` (module map, data flow provider→live config, proxy pipeline, DB migrations, i18n/locale pipeline), `docs/dev/testing.md`, and a root `AGENTS.md`/`CLAUDE.md` pointing to them; shrink the README architecture section to a link.
- **Acceptance:** README architecture block replaced by a link in all four languages; `docs/dev/architecture.md` exists and is referenced from CONTRIBUTING.

### DOCS-10: Guides, the session-manager PRD and the deep-link generator are not discoverable

**Type:** docs · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `docs/guides/` holds 19 files (6 guides × en/zh/ja + `proxy-guide-zh.md`) with no `README.md`; `rg -n "docs/guides" README*.md docs/user-manual/en/README.md` → 0 — guides are linked only from release notes (`docs/release-notes/v3.17.0-en.md:11-14`); `session-manager.md` (Chinese PRD, "v1 仅 macOS", `:1-4`) sits at the repo root unlinked (`rg session-manager.md README*.md docs` → 0); `deplink.html` likewise (DOCS-04); `README.md:320-322` "Documentation" section links only the user manual; `README_DE.md:322` links the English manual (no DE manual).
- **Problem:** High-value how-tos (Codex↔Claude routing, Kimi/DeepSeek routing, official-auth preservation) are invisible after the release week; a stale PRD at the root suggests macOS-only sessions.
- **Proposal:** Add `docs/guides/README.md` index (en/zh/ja) and link it from README "Documentation" and the manual index; move `session-manager.md` to `docs/dev/prd/` with a status banner or delete it; link the generator from the deep-link manual page and README.
- **Acceptance:** README "Documentation" lists Manual, Guides, Release notes, Deep-link spec/generator; repo root contains no orphan docs.

### DOCS-11: README front-loads 175 lines of sponsor banners before any product description

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `README.md:21-195` "❤️Sponsor" (60 `<img>/<a>` tags) precedes "Why CC Switch?" at `:196`, Screenshots `:210`, Features `:216`; same layout in ZH/JA/DE; the four READMEs are 589/592/589/589 lines with identical skeletons (11 FAQ `<details>`, 14 feature bullets each) and also duplicate dev commands and architecture (`:440-570`).
- **Problem:** New visitors scroll ~3 screens of ads before learning what the app does; four hand-synced copies of a 590-line document make structural drift likely (already: release-notes link, "Hermes" vs "Hermes Agent" at `README.md:5` vs `:257`).
- **Proposal:** Keep a compact sponsor strip (one row) near the top and move the full gallery to `SPONSORS.md` or below Features; move dev/architecture content to `docs/dev/` (DOCS-09) so each README is ~250 lines; generate the four READMEs from a template + per-language content files, or at least add a CI diff of heading structure across them.
- **Acceptance:** Product description visible within the first screen on GitHub; a structure-parity check passes for the four READMEs.

### DOCS-14: Import-from-existing-configs is silent and per-app; surface a cross-app import summary

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** Backend auto-import at startup (`src-tauri/src/app_config.rs:563,639,723-733` `default_with_auto_import`, `maybe_auto_import_prompts_for_existing_config`) has no UI feedback — the `notifications.autoImported` key ("Default provider created from existing configuration", `en.json:241`) is in the dead-key list; `ProviderEmptyState.tsx:38-45` offers "Import current" per app only; README `:342` "On first launch, you can manually import existing CLI tool configs as the default provider".
- **Problem:** Users do not learn what was imported (and from which file), so the FAQ item "My plugin configuration disappeared after switching providers" (`README.md:269-273`) keeps recurring.
- **Proposal:** After import, emit an event the frontend turns into a summary dialog/toast ("Imported Claude Code from ~/.claude/settings.json as 'default'; Codex: nothing found"), with a "Scan again" action in Settings → Advanced; reuse this in the wizard (DOCS-13).
- **Acceptance:** Fresh install with an existing `~/.claude/settings.json` shows the summary; `notifications.autoImported` is referenced from code.

### DOCS-15: Add a health checklist / diagnostics panel with "copy diagnostics"

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium

- **Evidence:** Health signals exist but are scattered: tool install/version/conflict diagnostics in About (`settings.toolDiagnose*`, `settings.toolConflict*`, `src/components/settings/AboutSection.tsx:1062`), env-var conflict warnings (`env.warning.description` "Found {{count}} environment variables that may override your configuration"), proxy status/takeover (`src/hooks/useProxyStatus.ts`), failover queue empty error (`src-tauri/src/commands/failover.rs:116` "故障转移队列为空…"), provider health badges (`src/components/providers/ProviderHealthBadge.tsx`), stream check (`streamCheck.*`); `.github/ISSUE_TEMPLATE/bug_report.yml` asks users to type version/OS manually; crash log location `src-tauri/src/lib.rs:228`.
- **Problem:** Troubleshooting requires visiting 4–5 panels; bug reports lack consistent environment data.
- **Proposal:** A "Health" view (or Settings section) listing checks with status and fix actions: CLI tools found/runnable, config dirs writable, env-var overrides, proxy listening/port free, takeover vs. failover consistency, last backup time, update available; plus "Copy diagnostics" (app version, OS, language, tool versions, proxy state, last 50 log lines, secrets redacted) and a link from the bug-report template.
- **Acceptance:** Health view renders all checks green on a clean install; "Copy diagnostics" output pasted into `bug_report.yml` fills the environment fields.

### DOCS-16: No contextual help links from the app to the manual

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `rg -n "user-manual|docs/" src --glob '*.{ts,tsx}'` matches no documentation link (only preset vendor URLs); outbound links are `https://ccswitch.io` (`src/App.tsx:1198`, `src/components/settings/AboutSection.tsx:870`) and the GitHub repo/releases (`AboutSection.tsx:440-446,882`); the UI already has 79 `<Tooltip>` usages, 92 Help/Info icons and 392 hint/description keys, i.e. rich inline copy with nowhere deeper to go; the manual is organised by feature (`docs/user-manual/en/README.md:5-45`) and localized (en/zh/ja).
- **Problem:** Users hit complex panels (proxy settings, failover queue, Claude Desktop routing, OMO, usage pricing) with only tooltips; the manual's existence is invisible from inside the app.
- **Proposal:** Add a `<HelpLink topic="proxy.failover" />` component that resolves to `https://ccswitch.io/docs/<lang>/<path>#anchor` (or the GitHub manual URL) with `ja→ja`, `zh/zh-TW→zh`, else `en`; place it in panel headers of the 10 most complex views; add a "?" header button opening the manual index; keep the topic→URL map in one file so docs moves are one edit.
- **Acceptance:** Each major panel header has a help link that opens the matching manual section in the active language with English fallback.

### DOCS-17: In-app "What's new" after updates

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/settings/AboutSection.tsx:429-450` `handleOpenReleaseNotes` opens `https://github.com/farion1231/cc-switch/releases/tag/<version>` externally (`settings.releaseNotes`); the updater is built in (`settings.updateAvailableShort`, `@tauri-apps/plugin-updater` in `package.json`); trilingual notes exist per version in `docs/release-notes/` with a consistent header (`v3.17.0-en.md:1-6` has zh/ja cross-links); `settings.viewReleaseNotes`/`viewCurrentReleaseNotes` keys exist but are unused (dead list).
- **Problem:** After an auto-update users see no summary of what changed (3.17 added a new header switcher and schema migration); the GitHub release body is Chinese-only (`.github/workflows/release.yml:613-627`).
- **Proposal:** At build time bundle a trimmed `whats-new/<version>.{en,zh,ja}.md` (first "Highlights" section of the release note, ≤ 1 KB) and show a dismissible "What's new in 3.17.0" dialog once when `settings.lastSeenVersion !== appVersion`; link "Full notes" to the website/GitHub; generate the GitHub release body from the en note with links to zh/ja.
- **Acceptance:** Upgrading from 3.16.5 to 3.17.0 shows the dialog once in the active language; the release body on GitHub is English with language links.

### DOCS-19: Error presentation: raw backend strings in toasts, mixed languages, no "copy details" or next step

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** 266 `toast.error/warning` calls; 8 pass the raw error (`src/hooks/useGlobalProxy.ts:78` `toast.error(error.message)`, `src/components/settings/LogConfigPanel.tsx:40` `toast.error(String(e))`, `RectifierConfigPanel.tsx:47,59`, `SkillStorageLocationSettings.tsx:64`, `src/components/usage/ModelsDevPickerDialog.tsx:239`, `PricingEditModal.tsx:84`, `src/components/providers/ProviderList.tsx:243`); 36 interpolate `{{error}}` with the untranslated backend text (e.g. `src/App.tsx:757-759` appends `: ${errorMessage}`); backend strings are Chinese (I18N-03); the skills domain already has structured errors with suggestions (`src/lib/errors/skillErrorParser.ts:15-60`, `skills.error.suggestion.*`).
- **Problem:** An English user can see "Save failed: 无效的 JSON 格式: expected `,` at line 3" with no action; error text is not copyable for bug reports.
- **Proposal:** Introduce `showError({ code | message, detail, action? })` presenter: localized title from the code, detail collapsed, "Copy details" button, optional action (open settings, open docs via DOCS-16); route all `toast.error` through it (lint ban on `toast.error(String(`/`error.message)`); feed from structured codes as I18N-03 lands.
- **Acceptance:** `rg "toast\.error\((String\(|error\.message|e\b)" src` → 0; error toasts show a localized title + copy button.

### DOCS-03: README screenshots show a three-app UI that predates most shipped features

**Type:** docs · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `README.md:210-214` embeds `assets/screenshots/main-en.png` and `add-en.png` (zh/ja variants for the other READMEs; no DE set); `main-en.png` (viewed) shows only Claude/Codex/Gemini tabs, no Grok Build/OpenCode/OpenClaw/Hermes/Claude Desktop, and no proxy, usage, skills, sessions or profile controls that `README.md:216-250` advertises; `docs/user-manual/assets/image-2026010800*.png` (22 files, timestamps 2026-01-08 in the names) predate v3.11+ features.
- **Problem:** The first visual impression under-sells the product and misleads users about the UI they will install; manual screenshots reference old layouts.
- **Proposal:** Regenerate `main`/`add` for en/zh/ja (+ dark mode) at 3.17.0 and add one proxy/usage dashboard shot; script it (Tauri + a `--screenshot` dev flag or Playwright against `pnpm dev:renderer` with MSW fixtures from `tests/msw`) so refreshes are cheap at each minor release; replace the timestamp-named manual assets with descriptive names.
- **Acceptance:** Screenshots show the 8-app switcher and the proxy/usage entry points; a `scripts/screenshots.*` command exists and is referenced in the release checklist.
- **Verification:** **partially-confirmed** — Confirmed: README.md:212-214 (not 210) embeds assets/screenshots/main-en.png and add-en.png, README_ZH.md:215 and README_JA.md:214 use zh/ja variants, README_DE.md:214 reuses the en pair; viewing main-en.png shows only Claude / Codex / Gemini tabs plus three toolbar icons and a provider list, with no project switcher, usage, proxy or Grok Build/OpenCode/OpenClaw/Hermes/Claude Desktop entries; docs/user-manual/assets holds 22 image-20260108*.png files whose timestamps fall between the 3.9.0 (2026-01-07) and 3.9.1 (2026-01-09) CHANGELOG dates, so they predate 3.10+. Mitigating detail: README.md:222 directly under the screenshot says '8 supported tools', so the mismatch is visible but self-correcting rather than misleading about functionality. The proposal to build a Tauri `--screenshot` flag or Playwright pipeline is over-engineered for refreshing six PNGs (though `dev:renderer` at package.json:10 and tests/msw do exist); stale marketing screenshots are a minor perception issue for a desktop app, so P3.

### DOCS-12: Cross-README consistency nits (relative `../../releases` links, naming, DE manual link)

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `linkcheck.py` flags 12 `[Releases](../../releases)` links (`README.md:354,372,386` and the same lines in ZH/JA/DE) — valid on GitHub, broken on mirrors, packagers (`flatpak/`, AUR) and the website; `README.md:5` "Hermes Agent" vs `:257` "Hermes" vs user-manual "Hermes"; `README_DE.md:322` links `docs/user-manual/en/README.md`; `README.md:218` version link (DOCS-02).
- **Problem:** Low-severity but visible inconsistencies, and a German reader is silently handed English docs with no note.
- **Proposal:** Use absolute `https://github.com/farion1231/cc-switch/releases` links; settle on one product name per tool in a glossary used by docs and `apps.*` locale keys; add "(English)" to the DE manual link.
- **Acceptance:** `linkcheck.py` treats `../../releases` as resolved (link absolute); glossary file exists and README headline matches FAQ naming.

### DOCS-13: First-run experience is a single text modal — add a guided setup wizard

**Type:** ux · **Priority:** P3 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src/components/FirstRunNoticeDialog.tsx:16-27` opens once when `firstRunNoticeConfirmed !== true` (backend decides at `src-tauri/src/lib.rs:560-561,673` only for a fresh install) and shows two paragraphs (`en.json:45-50`, naming only "Claude Code / Codex / Gemini CLI"); the main view then shows `ProviderEmptyState` per app (`src/components/providers/ProviderEmptyState.tsx:17-50`: Import current / Add provider); no wizard, checklist or tour component exists (`rg -i "wizard|onboarding|tour"` matches only the MCP wizard and the Claude `hasCompletedOnboarding` setting); visible apps default to all (`src-tauri/src/tray.rs:651`); existing building blocks: presets (`src/config/*Presets.ts`), endpoint test (`en.json` `endpointTest.*`), app visibility (`settings.appVisibility.*`), tool diagnostics (`settings.toolDiagnose*`).
- **Problem:** A new user must discover on their own which of 8 tools they use, that existing configs were imported as "default", how to add a key, and that terminals must be restarted (`README.md:262-264`); the welcome text is already stale.
- **Proposal:** A 4-step first-run wizard (skippable, re-openable from Settings): (1) pick tools you use → sets `visible_apps`; (2) detected existing configs with per-app import (reuse the backend auto-import, `src-tauri/src/app_config.rs:723-733`) and a clear "saved as default" message; (3) add first provider via preset + key with an inline endpoint test; (4) "what's next" cards (proxy/failover, MCP, usage, docs link). Replace `FirstRunNoticeDialog` with the wizard's intro step; keep the same `firstRunNoticeConfirmed` flag.
- **Acceptance:** Fresh install → wizard; completing it leaves ≥1 provider for each chosen app and hides unchosen apps; wizard strings exist in all locales; FirstRunNoticeDialog removed or reduced to step 1.
- **Verification:** **partially-confirmed** — Confirmed mechanics: src/components/FirstRunNoticeDialog.tsx:24 opens whenever `settings.firstRunNoticeConfirmed !== true`, src-tauri/src/lib.rs:560-564 and :673 gate that on a fresh install, en.json:47 names only 'Claude Code / Codex / Gemini CLI', src/components/providers/ProviderEmptyState.tsx:36-50 offers only Import/Add, and `rg -i 'wizard|onboarding|tour'` hits only McpWizardModal and the skipClaudeOnboarding setting. The problem statement is partly wrong: en.json:47-48 explicitly tells the user that existing config was saved as a provider named 'default' and that an 'Official' preset exists, so users do not have to discover that on their own; the cited src-tauri/src/app_config.rs:723-733 (`default_with_auto_import`) imports prompt files, not provider configs — the provider 'default' import is lib.rs:569-582 (`should_import_default_config_on_startup` / `import_default_config`); and visible apps do not default to all: src-tauri/src/settings.rs:51-62 sets every app true except `hermes: false`. The only concrete defect is the stale three-tool wording in the welcome text; the rest is an L-effort feature request for a wizard that the existing dialog + per-app empty state + backend auto-import already functionally cover, so P3 with medium confidence on the value of the proposal.

### DOCS-18: Keyboard shortcuts exist but are undocumented and platform-unaware

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** Handlers: Cmd/Ctrl+`,` opens Settings (`src/App.tsx:602-606`), Escape navigation (`:608`), Cmd/Ctrl+F focuses search in `src/components/providers/ProviderList.tsx:252`, `src/components/providers/forms/ProviderPresetSelector.tsx:183`, `src/components/workspace/DailyMemoryPanel.tsx:153`, Cmd+Enter submits the MCP wizard (`src/components/mcp/McpWizardModal.tsx:168`, macOS-only `metaKey`); the only user-facing mention is "⌘F" in `en.json:1979` (`workspace.dailyMemory.searchScopeHint`); the manual's 1.3-interface page does not list shortcuts; 25 keyboard-handling lines overall.
- **Problem:** Discoverability is nil; "⌘F" is shown to Windows/Linux users; `McpWizardModal` ignores Ctrl+Enter on Windows/Linux.
- **Proposal:** Centralise shortcuts in a `shortcuts.ts` registry (id, keys per platform, i18n label), render a "?"/Cmd+/ cheat-sheet overlay from it, show platform-correct glyphs (`⌘` vs `Ctrl`), fix `McpWizardModal` to accept `ctrlKey`, and document the list in the manual 1.3 page.
- **Acceptance:** Pressing `?`/Cmd+/ shows all registered shortcuts; `rg "⌘" src/i18n/locales/en.json` → 0 (glyphs come from the registry).

### DOCS-20: Per-OS troubleshooting is scattered and lacks log locations

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** Linux Wayland/NVIDIA issue lives in `README.md:308-318` and `docs/user-manual/en/5-faq/5.2-questions.md:32-47`; Windows "doesn't launch" at `5.2-questions.md:9-18`, macOS at `:5-8`, tray icon `:222-232`, update failed `:240-246`, "Log Files" `:276` — but `rg "crash.log" docs/user-manual/en` → 0 although `src-tauri/src/lib.rs:228` documents `~/.cc-switch/crash.log`; `README.md:346-396` install section has no troubleshooting pointers; `.github/ISSUE_TEMPLATE/bug_report.yml` asks for OS but links no per-OS guide.
- **Problem:** Users and triagers lack one place per OS with symptoms → fix → logs to attach.
- **Proposal:** Add `docs/user-manual/*/5-faq/5.5-troubleshooting.md` (Windows/macOS/Linux sections: install/launch, tray, updater, proxy port, log and crash-log paths, how to run from a terminal for stderr) and link it from the bug-report template and the README install section.
- **Acceptance:** Bug-report template links the page; the page lists `crash.log` and the proxy log path for each OS.

### DOCS-21: Issue/PR templates do not reflect four UI locales or the German README

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `.github/ISSUE_TEMPLATE/doc_issue.yml:21-28` language dropdown: English / 中文 / 日本語 (no 繁體中文 UI, no Deutsch README); `bug_report.yml` has no "app language" field (`rg -i "language|locale" bug_report.yml` → none); `.github/pull_request_template.md:25` single i18n checkbox; `labeler.yml:23-26` wrong path (I18N-05).
- **Problem:** Locale-specific bugs (e.g. the zh-TW gap in I18N-04 or Chinese fallbacks in I18N-01) cannot be triaged by language; doc issues for the DE README have no category.
- **Proposal:** Add 繁體中文 and Deutsch to the dropdown, add an "App language" dropdown (en/zh/zh-TW/ja) to `bug_report.yml`, expand the PR checklist to "all four locale files updated and `pnpm i18n:check` passes".
- **Acceptance:** Templates list all locales; new bug reports carry the app language.

### DOCS-22: GitHub release body is Chinese-only and hand-written in the workflow

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `.github/workflows/release.yml:608-627` `softprops/action-gh-release` with an inline `body:` containing "Claude Code 供应商切换工具", "### 下载", "（推荐）/（安装版）/（绿色版）" — only the first line is trilingual; `docs/release-notes/<ver>-{en,zh,ja}.md` already exist with cross-links (`v3.17.0-en.md:5`); `prerelease: true` by default.
- **Problem:** The Releases page — the primary download surface linked from all READMEs and SECURITY.md — reads as Chinese-only to international users and does not link the detailed notes.
- **Proposal:** Generate the body from `docs/release-notes/${tag}-en.md` (highlights) + a trilingual download table + links to the zh/ja notes, failing the workflow if the note for the tag is missing (which also enforces DOCS-02 coverage).
- **Acceptance:** A tag without `docs/release-notes/<tag>-en.md` fails the release job; the published body is English with language links and the download table.

### DOCS-23: Package/store descriptions and in-app product copy still describe a three-tool product

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `package.json:4` "All-in-One Assistant for Claude Code, Codex & Gemini CLI"; `flatpak/com.ccswitch.desktop.metainfo.xml:5,10-15` same wording, only Claude Code/Codex/Gemini; `src/i18n/locales/en.json:47` first-run copy names three tools; `docs/user-manual/README.md:3` seven tools; `README.md:5,257` eight.
- **Problem:** Four different tool lists across surfaces; store metadata and first-run copy are the ones new users read first.
- **Proposal:** Define the canonical tool list once (`src/config/apps.ts` or a docs glossary) and update package.json, metainfo (with `xml:lang` translations), first-run copy and manual index from it; add a CI grep guard for the old slogan.
- **Acceptance:** All surfaces list the same eight tools; `rg -n "Codex & Gemini CLI" package.json flatpak docs` → 0.
