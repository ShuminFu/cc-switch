# CC Switch — Next-Phase Proposals

_A consolidated backlog of bugs, security findings, refactors, performance work, test/CI gaps and product features, produced by 16 parallel code reviews of the repository at v3.17.0 (commit `613fef7`). Every item cites the code it was derived from; priorities and efforts are proposals for triage, not commitments._

## Executive summary

CC Switch at v3.17.0 is a large, fast-moving codebase (about 148k lines of Rust and 81k lines of TypeScript, eight managed tools, 285 Tauri commands, four proxy protocol bridges) that ships weekly and whose users trust it with their API keys. Sixteen independent reviews read every area of it; their findings were then re-checked by adversarial verifiers and extended by completeness critics. The picture that emerges is consistent across areas: the product surface is far ahead of its foundations, and the same handful of structural gaps explain most of the individual bugs.

**Five root causes account for the majority of the findings the reviewers rated highest.** (The verification pass re-rated most of those items from P1 to P2, judging them real but not emergencies; the structural picture is unchanged.)

1. **The SSOT ↔ live-file invariant is not transactional.** Provider switches persist pointers before the live write succeeds, several adapters overwrite live files without a backup, `atomic_write` is neither crash-safe nor atomic on Windows, and form state in the provider dialogs can silently discard edits. One failed write can later backfill the wrong provider's credentials into the database.
2. **Secrets are plaintext everywhere they rest or travel.** API keys live in SQLite, in `~/.cc-switch/backups`, in SQL exports and in the cloud-sync artifact; the sync importer executes arbitrary SQL from that artifact; the proxy can bind to all interfaces with no authentication; prompts, bodies and keys reach the on-disk log.
3. **Per-app logic exists as eight hand-maintained copies.** Live read/merge/write/backfill, MCP sync, app-identity lists, label maps, tray menus and provider forms are each duplicated per tool with drift between copies (adding Grok Build cost 112 files). A conformance-tested `AppAdapter` trait is the single most leveraged refactor in this backlog.
4. **The proxy is a 4.8k-line forwarder plus thirteen pairwise codecs with no end-to-end test.** Fixes land in one bridge and not its siblings, a HalfOpen permit can leak, the header-preserving path opens a new TLS connection per request, and `forward()` cannot be tested without a Tauri app handle. The changelog shows this is already the number-one fix hotspot.
5. **One `std::sync::Mutex<Connection>` with no WAL serialises everything.** The proxy's per-request usage logging, 60-second session imports, dashboard aggregates on the main thread, startup maintenance and the tray rebuild all contend for the same lock, and much of that work runs synchronously on Tokio or the Tauri main thread.

Around those cores sit well-evidenced gaps in testing (no proxy harness, no command-layer tests, no coverage gate, 22% MSW handler coverage), release integrity (unsigned Windows installers, an updater endpoint that cannot see prereleases, hand-bumped versions in four files), internationalization (Chinese fallbacks for non-CJK locales, Chinese-only backend errors matched by substring), documentation (manual frozen at v3.16.0) and product (budgets, notifications, health probing and a CLI are all half-built or documented but missing).

The recommendation is to run the next phase as **a foundations phase with a visible product payoff at the end of it**: fix the data-loss and secrets issues first, land the five enabling refactors (adapter trait, DB writer model, proxy harness and IR, typed IPC, frontend shell), and only then open the product themes ("proxy as a product", "trust and governance", "projects everywhere"), which all depend on those foundations.

## Recommended phase plan

The sequence below is ordered by dependency, not by appeal: every product theme in Phase 3 needs at least one foundation from Phases 1 and 2. Effort letters follow the findings (S under a day, M one to three days, L one to two weeks, XL multi-week).

### Phase 0 — Stop the bleeding (first two weeks)

Small, independent fixes with outsized risk. Each is S or M effort and most carry a regression test.

- Config integrity: SVC-01 (switch pointer order), SVC-06, SVC-07, SVC-08, CFG-02, CFG-03, CFG-04, CMD-06, FE-PROV-01, FE-PROV-02, FE-PROV-04, FE-PROV-05, FE-UI-01, FE-UI-02, FE-UI-03.
- Secrets and exposure: DB-01 (sync importer SQL), SEC-01, PROXY-03 and PRODUCT-02 (listener auth on non-loopback), SEC-02, SEC-03, SEC-04, SEC-07, SEC-08, PROXY-13, PROXY-33, CMD-01, CMD-02, SESS-05, SESS-06, SVC-03 (zip-slip), CFG-05, SESS-01, SESS-02.
- Proxy correctness: PROXY-01, PROXY-02, PROXY-10, PROV-01, PROV-02, PROV-05.
- Database quick wins: PERF-02 (WAL, `synchronous=NORMAL`, `busy_timeout`), DB-10.
- Release and i18n hygiene: CI-02, CI-03, CI-05, CI-09, I18N-01, I18N-02, I18N-04, I18N-06, DOCS-02.

### Phase 1 — Foundations (weeks three to eight)

The enabling refactors. They are ordered so that each one de-risks the next.

1. **Test and CI backbone** (TEST-01, TEST-02, TEST-04, TEST-06, TEST-08, PROXY-06, PROV-20, CI-08, CI-10, CI-11, FE-ARCH-07, I18N-05): an in-process proxy harness with a mock upstream, a golden-fixture corpus for the bridges, MSW handlers generated from the command registry, coverage gates, ESLint, dependency and license scanning. Without this layer the refactors below cannot be landed safely.
2. **Database connection model** (DB-03, DB-04, DB-05, DB-07, DB-08, DB-09, PERF-03, PERF-17, PROXY-04): WAL plus a dedicated writer task with batched usage ingestion, read connections for the UI, `spawn_blocking` for every SQLite call, one migration ledger with checksums replacing the two parallel schema paths.
3. **`AppAdapter` trait and shared live-file layer** (CFG-06, SVC-18, FE-ARCH-10, FE-UI-04, FE-PROV-22): one trait per tool over a format-preserving document layer with transactional writes and backups, a conformance suite every adapter must pass, and a single app-capability registry shared by frontend and backend.
4. **Typed IPC and structured errors** (CMD-12, CMD-13, FE-ARCH-01, FE-ARCH-03, TEST-05, I18N-03): generate the TypeScript bindings from Rust with tauri-specta, return an `AppError` code enum instead of Chinese strings, delete the 291 hand-written wrappers and the substring matching.
5. **Async hygiene and startup** (CMD-07, CMD-08, CMD-09, SVC-05, PERF-11, PERF-12, PERF-16): remove `block_on` from command paths, make heavy commands async, show the window before imports and maintenance, rebuild the tray incrementally.
6. **Release integrity** (CI-01, CI-04, CI-06, CI-07, PRODUCT-35): Windows signing, `latest.json` validation, single version source with a bump script, dry-run releases, SBOM and provenance attestations.

### Phase 2 — Architecture (weeks nine to sixteen)

Larger restructurings that become tractable once Phase 1 has landed.

- **Proxy pipeline and canonical IR** (PROXY-07, PROV-19, PROXY-05, PROXY-08, PROXY-18, PERF-01, PERF-04, PERF-05): a staged request pipeline with a pooled transport and one timeout policy, a canonical message/stream representation with one codec per protocol, usage logging implemented once.
- **Frontend shell** (FE-ARCH-02, FE-ARCH-04, FE-ARCH-05, FE-ARCH-08, FE-PROV-17, FE-UI-16, FE-UI-17, FE-UI-18, PERF-13, PERF-14, PERF-15): a router-driven shell instead of the 1,673-line `App.tsx`, per-app form adapters instead of the 2.6k-line `ProviderFormFull`, a shared managed-items panel for MCP/Skills/Prompts, centralised query keys, event-driven invalidation instead of polling, route-level code splitting.
- **Sync protocol v2** (SVC-02, SVC-04, SVC-19, SVC-24, DB-06, PRODUCT-01): a `SyncTransport` trait over WebDAV and S3, remote preconditions and local-dirty detection instead of last-writer-wins, device-level tables excluded from sync, encrypted bundles with keys held in the OS keychain.
- **Session index and deep-link v2** (SESS-03, SESS-04, SESS-09, SESS-10, SESS-12, DOCS-04): a persistent, mtime-indexed session catalogue with full-text search and a typed terminal launcher; a backend-held pending import with full-disclosure confirmation, a published link spec, signed and batch links.
- **Workspace split** (CI-38, CMD-10): split the 147k-line crate into core, proxy, CLI and Tauri shell so unit tests no longer need GTK or WebKit and a headless daemon becomes possible.

### Phase 3 — Product themes (from week twelve, overlapping Phase 2)

- **Proxy as a product** (PRODUCT-61, PRODUCT-03, PRODUCT-06, PRODUCT-10, PRODUCT-11, PROXY-19, PROXY-20, PROXY-21, PROXY-22): proactive health probing, tray and system notifications, routing strategies, a headless daemon and a `ccs` CLI over the same store, metrics and a request flight recorder.
- **Trust and governance** (PRODUCT-62, PRODUCT-07, PRODUCT-08, PRODUCT-09, PRODUCT-54): keychain vault, signed remote preset and sponsor catalog, in-app affiliate disclosure, a public sponsor policy, a gate and honest wording for terms-of-service-sensitive features.
- **Projects everywhere** (PRODUCT-63, PRODUCT-12, PRODUCT-05): per-directory profile binding, all eight apps in profiles, tray, universal providers and takeover, budgets and alerts, usage attribution by project.
- **Onboarding, docs and i18n process** (DOCS-01, DOCS-03, DOCS-13, I18N-05, I18N-03): a guided setup wizard, a refreshed manual and screenshots, a locale-parity check in CI, typed translation keys, localized backend errors.

### How to use this backlog

Every finding carries a verification verdict. Schedule `confirmed` and `partially-confirmed` items directly (the notes say what was corrected). Treat `unverifiable` items as spikes. Do not schedule `refuted` items; they are kept so reviewers can see what was checked. `critic-added` items were found by the second pass and have not been independently re-verified, so budget a short confirmation before committing to them. Line numbers refer to commit `613fef7`.

## Cross-cutting themes

Each theme groups findings from several areas that share one root cause or one enabling change. IDs link to the area files.

### Config integrity

Make the SSOT ↔ live-file invariant transactional: write live first, then persist pointers; back up before overwrite; crash-safe atomic writes; forms that never discard edits.

Findings (P1 3, P2 12, P3 2): SVC-01, SVC-06, SVC-07, SVC-08, CFG-02, CFG-03, CFG-04, CFG-07, CFG-08, CMD-06, DB-10, FE-PROV-01, FE-PROV-02, FE-PROV-04, FE-PROV-05, FE-UI-01, FE-UI-02

### Secrets and exposure

Keys out of plaintext and out of logs; no arbitrary SQL from sync artifacts; authenticate the proxy off loopback; stop handing bearer tokens to the webview; tighten CSP and capabilities.

Findings (P1 11, P2 19, P3 3): DB-01, DB-02, SVC-04, CMD-01, CMD-02, CMD-03, CMD-04, PROXY-03, PROXY-13, PROXY-33, SESS-05, SESS-06, CFG-01, CFG-05, SESS-01, SESS-02, SVC-03, PRODUCT-01, PRODUCT-02, SEC-01, SEC-02, SEC-03, SEC-04, SEC-05, SEC-06, SEC-07, SEC-08, SEC-10, SEC-11, SEC-12, SEC-13, SEC-16, SEC-17

### Cloud sync v2

Replace last-writer-wins with preconditions and dirty detection, exclude device-level state, unify WebDAV and S3 behind one transport, encrypt bundles.

Findings (P1 1, P2 5, P3 1): SVC-02, SVC-19, SVC-24, DB-06, DB-08, SVC-04, SEC-06

### One adapter per app

An AppAdapter trait over a format-preserving document layer, a conformance suite every tool must pass, and one capability registry shared by Rust and TypeScript, so the ninth tool costs a module instead of 112 files.

Findings (P2 9, P3 1): CFG-06, SVC-18, FE-ARCH-10, FE-UI-04, FE-PROV-17, FE-PROV-22, CMD-10, CFG-10, PRODUCT-64, CI-38

### Proxy reliability

RAII guards for breaker permits, one transport with pooling and one timeout policy, stream failures fed back to health, then a staged pipeline that can be tested without a Tauri handle.

Findings (P1 1, P2 13): PROXY-01, PROXY-02, PROXY-05, PROXY-08, PROXY-09, PROXY-10, PROXY-07, PROXY-06, PERF-01, PROXY-22, PRODUCT-03, PROXY-14, SEC-14, SEC-15

### Format conversion

A canonical message and stream IR with one codec per protocol, backed by recorded golden fixtures and round-trip property tests, so a fix lands once instead of in one of thirteen converters.

Findings (P1 2, P2 8, P3 1): PROV-01, PROV-02, PROV-03, PROV-04, PROV-05, PROV-06, PROV-19, PROV-20, TEST-01, TEST-02, PRODUCT-04

### Database model

WAL and busy_timeout today; a dedicated writer task with batched usage ingestion, read connections for the UI, spawn_blocking everywhere, and one migration ledger tomorrow.

Findings (P1 1, P2 9, P3 2): PERF-02, DB-03, DB-04, DB-05, DB-07, DB-08, DB-09, PERF-03, PERF-08, PERF-09, PERF-17, PROXY-04

### App shell and IPC

Generated bindings and a structured error enum across the Rust/TypeScript boundary, no block_on on command paths, async heavy commands, and a startup that shows the window before it imports.

Findings (P1 1, P2 11, P3 1): CMD-07, CMD-08, CMD-09, CMD-12, CMD-13, SVC-05, FE-ARCH-01, FE-ARCH-03, TEST-05, I18N-03, PERF-11, PERF-12, PERF-16

### Frontend shell and forms

A router instead of a 1,673-line App.tsx, per-app form adapters instead of a 2.6k-line ProviderFormFull, one managed-items panel, centralised query keys, events instead of polling, lazy routes.

Findings (P2 13): FE-ARCH-02, FE-ARCH-04, FE-ARCH-05, FE-ARCH-06, FE-ARCH-08, FE-UI-05, FE-UI-16, FE-UI-17, FE-UI-18, FE-PROV-14, PERF-13, PERF-14, PERF-15

### Test pyramid and CI gates

An in-process proxy harness, command-layer tests, MSW handlers generated from the registry, coverage gates, ESLint, vulnerability and license scanning, and a real bundle build on pull requests.

Findings (P1 1, P2 13, P3 1): TEST-01, TEST-03, TEST-04, TEST-06, TEST-07, TEST-08, TEST-33, FE-UI-42, SVC-25, CMD-31, CI-08, CI-10, CI-11, FE-ARCH-07, I18N-05

### Release integrity

Sign Windows builds, make updater signatures mandatory and validated, keep secrets out of shell text, derive the version from one source, rehearse releases with a dry run.

Findings (P2 11): CI-01, CI-02, CI-03, CI-04, CI-05, CI-06, CI-07, CI-09, CI-37, PRODUCT-35, SEC-18

### Sessions and deep links

A persistent session index with full-text search and a typed terminal launcher; a deep-link trust model with backend-held pending imports, full disclosure, a published spec and signed or batched links.

Findings (P2 7, P3 2): SESS-03, SESS-04, SESS-09, SESS-10, SESS-12, SESS-08, DOCS-04, SEC-21, SEC-24

### Proxy as a product

Proactive health probing, tray and system notifications, routing strategies, a headless daemon and a ccs CLI over the same store, metrics and a request flight recorder.

Findings (P2 8): PRODUCT-61, PRODUCT-06, PRODUCT-10, PRODUCT-11, PROXY-19, PROXY-20, PROXY-21, PROXY-35

### Trust and governance

A keychain vault, a signed remote preset and sponsor catalog, in-app affiliate disclosure, a public sponsor policy, and honest gating of terms-of-service-sensitive features.

Findings (P1 1, P2 4): PRODUCT-62, PRODUCT-07, PRODUCT-08, PRODUCT-09, PRODUCT-54

### Projects everywhere

Per-directory profile binding, all eight apps in profiles, tray, universal providers and takeover, budgets and alerts, usage attribution by project.

Findings (P1 1, P2 2): PRODUCT-63, PRODUCT-12, PRODUCT-05

### Onboarding, docs and i18n

Fix Chinese fallbacks for non-CJK users and the ten missing keys, add a locale-parity check, refresh the manual and screenshots, publish the deep-link spec, add a guided setup wizard.

Findings (P2 6, P3 4): I18N-01, I18N-02, I18N-04, I18N-06, DOCS-01, DOCS-02, DOCS-03, DOCS-13, FE-PROV-03, PROXY-31

## Backlog at a glance

**709 findings** across 17 areas. P0: 0 · P1: 27 · P2: 440 · P3: 242.

| Area | File | Findings | P0 | P1 | P2 | P3 | Top types |
|---|---|---|---|---|---|---|---|
| Security & privacy | [security.md](findings/security.md) | 27 | 0 | 4 | 15 | 8 | security 22, privacy 3, bug 1 |
| Proxy core | [proxy-core.md](findings/proxy-core.md) | 48 | 0 | 1 | 27 | 20 | bug 25, security 5, performance 5 |
| Proxy providers & format conversion | [proxy-providers.md](findings/proxy-providers.md) | 43 | 0 | 0 | 28 | 15 | bug 20, feature 6, refactor 5 |
| Backend services | [backend-services.md](findings/backend-services.md) | 53 | 0 | 4 | 32 | 17 | bug 25, tech-debt 7, refactor 5 |
| Commands / IPC / app shell | [commands-app-shell.md](findings/commands-app-shell.md) | 47 | 0 | 1 | 33 | 13 | bug 14, refactor 7, security 6 |
| Per-app config adapters & MCP sync | [config-adapters.md](findings/config-adapters.md) | 48 | 0 | 0 | 31 | 17 | bug 25, tech-debt 5, feature 5 |
| Database / DAO / migrations | [database.md](findings/database.md) | 44 | 0 | 2 | 25 | 17 | bug 14, performance 8, tech-debt 8 |
| Session manager, deep links & usage ingestion | [sessions-deeplinks.md](findings/sessions-deeplinks.md) | 34 | 0 | 2 | 17 | 15 | bug 11, security 7, feature 5 |
| Frontend architecture | [frontend-architecture.md](findings/frontend-architecture.md) | 36 | 0 | 0 | 22 | 14 | tech-debt 11, refactor 7, feature 5 |
| Provider management UI | [frontend-providers.md](findings/frontend-providers.md) | 55 | 0 | 2 | 28 | 25 | bug 27, ux 9, feature 7 |
| Feature UI (settings, usage, proxy, skills, MCP) | [frontend-features.md](findings/frontend-features.md) | 47 | 0 | 0 | 23 | 24 | bug 14, ux 11, feature 8 |
| Performance & resources | [performance.md](findings/performance.md) | 40 | 0 | 2 | 27 | 11 | performance 34, tech-debt 3, refactor 2 |
| Testing & QA | [testing.md](findings/testing.md) | 33 | 0 | 1 | 23 | 9 | test 22, tech-debt 4, ci 3 |
| CI/CD, build, release & dependencies | [ci-build-release.md](findings/ci-build-release.md) | 38 | 0 | 0 | 34 | 4 | tech-debt 7, security 6, ci 5 |
| Internationalization | [i18n.md](findings/i18n.md) | 20 | 0 | 1 | 13 | 6 | bug 7, tech-debt 4, feature 3 |
| Documentation & onboarding | [docs-onboarding.md](findings/docs-onboarding.md) | 23 | 0 | 0 | 15 | 8 | docs 16, feature 4, ux 3 |
| Product strategy & feature roadmap | [product-roadmap.md](findings/product-roadmap.md) | 73 | 0 | 7 | 47 | 19 | feature 37, bug 10, risk 9 |

**By type:** bug 201 · feature 98 · performance 75 · tech-debt 72 · refactor 62 · security 59 · test 41 · ux 39 · docs 37 · risk 12 · ci 10 · privacy 3

## Verification pass

Every finding that the first pass rated P0 or P1 (157 items) was handed to an independent verifier that re-read the cited code and tried to refute it. Verdicts: partially-confirmed 100 · critic-added 86 · confirmed 53 · refuted 1. The verifiers were asked to rate by realistic likelihood and impact rather than by importance to the roadmap, and they lowered 141 ratings and raised 0 (P0→P1: 11, P0→P2: 1, P1→P2: 115, P1→P3: 14). Both ratings are kept: the corrected one drives the tables below, the first-pass one is shown beside it, and the verifier's notes sit under **Verification** in each area file. Read a downgrade as 'real, but not an emergency', not as 'wrong': the verifiers confirmed the underlying observation in all but one case. Findings the first pass rated P2 or P3 were not re-verified.

## P1 — high priority after verification

27 findings.

| ID | Area | Title | Type | Effort | Verdict |
|---|---|---|---|---|---|
| [SEC-01](findings/security.md#sec-01) | Security & privacy | Local proxy injects stored API keys for any unauthenticated client (local process, CSRF from web pages, LAN when bound to 0.0.0.0) | security | M | confirmed |
| [SEC-02](findings/security.md#sec-02) | Security & privacy | Deep link `enabled=true` makes an attacker-supplied endpoint the *current* provider after one click; the flag is not shown in the confirmation dialog | security | S | confirmed |
| [SEC-03](findings/security.md#sec-03) | Security & privacy | MCP deep link writes arbitrary stdio servers (command + args + env) into live tool configs; the dialog hides args/env and the "written immediately" warning is shown only when a parameter the backend ignores is set | security | S | confirmed |
| [SEC-04](findings/security.md#sec-04) | Security & privacy | Raw deep-link URLs (including `apiKey`, `usageAccessToken`, `usageApiKey`) are written to the plaintext log file | security | S | partially-confirmed |
| [PROXY-03](findings/proxy-core.md#proxy-03) | Proxy core | Proxy can be bound to all interfaces with no authentication on the main API routes | security | M | confirmed |
| [SVC-01](findings/backend-services.md#svc-01) | Backend services | Provider switch persists "current" pointers before the live write, and a failed write poisons the next backfill | bug | M | partially-confirmed |
| [SVC-02](findings/backend-services.md#svc-02) | Backend services | Cloud sync is last-writer-wins: no remote precondition on upload and no local-dirty check on download | bug | L | partially-confirmed |
| [SVC-08](findings/backend-services.md#svc-08) | Backend services | Gemini `settings.json` merge swallows parse errors and overwrites the file with provider-only content | bug | S | confirmed |
| [SVC-42](findings/backend-services.md#svc-42) | Backend services | Cloud sync replicates device-local takeover/proxy state and profile pointers; a download silently enables proxy takeover on the other device | bug | M |  |
| [CMD-40](findings/commands-app-shell.md#cmd-40) | Commands / IPC / app shell | Cold-start and lightweight-mode deep links are silently dropped (no get_current()/pending buffer) | bug | S |  |
| [DB-01](findings/database.md#db-01) | Database / DAO / migrations | Sync importer executes arbitrary SQL from the remote artifact after a header-only check | security | L | confirmed |
| [DB-02](findings/database.md#db-02) | Database / DAO / migrations | Provider API keys, backups and the cloud sync artifact are all plaintext | security | XL | partially-confirmed |
| [SESS-02](findings/sessions-deeplinks.md#sess-02) | Session manager, deep links & usage ingestion | MCP deep links write arbitrary `command/args/env` into live tool configs; `enabled` is ignored and the confirmation hides args/env | security | M | confirmed |
| [SESS-05](findings/sessions-deeplinks.md#sess-05) | Session manager, deep links & usage ingestion | Raw deep-link URL (including `apiKey`) is logged at info level and shipped in the `deeplink-error` payload | security | S | confirmed |
| [FE-PROV-01](findings/frontend-providers.md#fe-prov-01) | Provider management UI | JSON-editor edits are silently overwritten by the next field edit (stale `getValues` snapshots) | bug | M | confirmed |
| [FE-PROV-06](findings/frontend-providers.md#fe-prov-06) | Provider management UI | OMO custom agent/category rows vanish before a model is chosen | bug | S | confirmed |
| [PERF-01](findings/performance.md#perf-01) | Performance & resources | Header-case-preserving upstream path performs a full TCP + TLS handshake on every request (no connection reuse) | performance | L | confirmed |
| [PERF-02](findings/performance.md#perf-02) | Performance & resources | SQLite opened with default journal/sync settings behind one global std Mutex (no WAL, synchronous=FULL, no busy_timeout, no cache tuning) | performance | S | partially-confirmed |
| [TEST-01](findings/testing.md#test-01) | Testing & QA | No in-process end-to-end test of the proxy request path | test | L | confirmed |
| [I18N-03](findings/i18n.md#i18n-03) | Internationalization | Backend user-facing errors are Chinese-only strings matched by substring on the frontend | tech-debt | XL | confirmed |
| [PRODUCT-01](findings/product-roadmap.md#product-01) | Product strategy & feature roadmap | Secrets vault: OS keychain for API keys/tokens and encrypted export & cloud-sync bundles | risk | L | partially-confirmed |
| [PRODUCT-02](findings/product-roadmap.md#product-02) | Product strategy & feature roadmap | Authenticate the proxy listener when bound to a non-loopback address | risk | S | confirmed |
| [PRODUCT-04](findings/product-roadmap.md#product-04) | Product strategy & feature roadmap | Proxy bridge conformance suite (golden fixtures per protocol pair) | refactor | M | partially-confirmed |
| [PRODUCT-62](findings/product-roadmap.md#product-62) | Product strategy & feature roadmap | THEME — "Trust & governance" (vault, listener auth, signed catalog, disclosure, policy, audit, signing) | risk | XL | partially-confirmed |
| [PRODUCT-63](findings/product-roadmap.md#product-63) | Product strategy & feature roadmap | THEME — "Projects everywhere" (per-directory binding, all apps in profiles/tray/universal/takeover, usage by project) | feature | XL | partially-confirmed |
| [PRODUCT-65](findings/product-roadmap.md#product-65) | Product strategy & feature roadmap | THEME — "Reliability engineering" (bridge fixtures, e2e scenarios, beta ring, diagnostics) | refactor | L | partially-confirmed |
| [PRODUCT-66](findings/product-roadmap.md#product-66) | Product strategy & feature roadmap | Database backup restore skips the post-import live sync, settings reload and takeover reconciliation that import/WebDAV/S3 all run | bug | S |  |

## Everything the first pass rated P0 or P1, with the verifier's correction

157 findings. Sorted by corrected priority. The verifier's notes are in the area files under **Verification**.

| ID | Area | Title | First pass | Verified | Verdict | Effort |
|---|---|---|---|---|---|---|
| [SEC-01](findings/security.md#sec-01) | Security & privacy | Local proxy injects stored API keys for any unauthenticated client (local process, CSRF from web pages, LAN when bound to 0.0.0.0) | P1 | P1 | confirmed | M |
| [SEC-02](findings/security.md#sec-02) | Security & privacy | Deep link `enabled=true` makes an attacker-supplied endpoint the *current* provider after one click; the flag is not shown in the confirmation dialog | P1 | P1 | confirmed | S |
| [SEC-03](findings/security.md#sec-03) | Security & privacy | MCP deep link writes arbitrary stdio servers (command + args + env) into live tool configs; the dialog hides args/env and the "written immediately" warning is shown only when a parameter the backend ignores is set | P1 | P1 | confirmed | S |
| [SEC-04](findings/security.md#sec-04) | Security & privacy | Raw deep-link URLs (including `apiKey`, `usageAccessToken`, `usageApiKey`) are written to the plaintext log file | P1 | P1 | partially-confirmed | S |
| [PROXY-03](findings/proxy-core.md#proxy-03) | Proxy core | Proxy can be bound to all interfaces with no authentication on the main API routes | P1 | P1 | confirmed | M |
| [SVC-01](findings/backend-services.md#svc-01) | Backend services | Provider switch persists "current" pointers before the live write, and a failed write poisons the next backfill | P0 | P1 | partially-confirmed | M |
| [SVC-02](findings/backend-services.md#svc-02) | Backend services | Cloud sync is last-writer-wins: no remote precondition on upload and no local-dirty check on download | P0 | P1 | partially-confirmed | L |
| [SVC-08](findings/backend-services.md#svc-08) | Backend services | Gemini `settings.json` merge swallows parse errors and overwrites the file with provider-only content | P1 | P1 | confirmed | S |
| [SVC-42](findings/backend-services.md#svc-42) | Backend services | Cloud sync replicates device-local takeover/proxy state and profile pointers; a download silently enables proxy takeover on the other device | P1 | P1 |  | M |
| [CMD-40](findings/commands-app-shell.md#cmd-40) | Commands / IPC / app shell | Cold-start and lightweight-mode deep links are silently dropped (no get_current()/pending buffer) | P1 | P1 |  | S |
| [DB-01](findings/database.md#db-01) | Database / DAO / migrations | Sync importer executes arbitrary SQL from the remote artifact after a header-only check | P0 | P1 | confirmed | L |
| [DB-02](findings/database.md#db-02) | Database / DAO / migrations | Provider API keys, backups and the cloud sync artifact are all plaintext | P0 | P1 | partially-confirmed | XL |
| [SESS-02](findings/sessions-deeplinks.md#sess-02) | Session manager, deep links & usage ingestion | MCP deep links write arbitrary `command/args/env` into live tool configs; `enabled` is ignored and the confirmation hides args/env | P1 | P1 | confirmed | M |
| [SESS-05](findings/sessions-deeplinks.md#sess-05) | Session manager, deep links & usage ingestion | Raw deep-link URL (including `apiKey`) is logged at info level and shipped in the `deeplink-error` payload | P1 | P1 | confirmed | S |
| [FE-PROV-01](findings/frontend-providers.md#fe-prov-01) | Provider management UI | JSON-editor edits are silently overwritten by the next field edit (stale `getValues` snapshots) | P1 | P1 | confirmed | M |
| [FE-PROV-06](findings/frontend-providers.md#fe-prov-06) | Provider management UI | OMO custom agent/category rows vanish before a model is chosen | P1 | P1 | confirmed | S |
| [PERF-01](findings/performance.md#perf-01) | Performance & resources | Header-case-preserving upstream path performs a full TCP + TLS handshake on every request (no connection reuse) | P0 | P1 | confirmed | L |
| [PERF-02](findings/performance.md#perf-02) | Performance & resources | SQLite opened with default journal/sync settings behind one global std Mutex (no WAL, synchronous=FULL, no busy_timeout, no cache tuning) | P0 | P1 | partially-confirmed | S |
| [TEST-01](findings/testing.md#test-01) | Testing & QA | No in-process end-to-end test of the proxy request path | P0 | P1 | confirmed | L |
| [I18N-03](findings/i18n.md#i18n-03) | Internationalization | Backend user-facing errors are Chinese-only strings matched by substring on the frontend | P1 | P1 | confirmed | XL |
| [PRODUCT-01](findings/product-roadmap.md#product-01) | Product strategy & feature roadmap | Secrets vault: OS keychain for API keys/tokens and encrypted export & cloud-sync bundles | P0 | P1 | partially-confirmed | L |
| [PRODUCT-02](findings/product-roadmap.md#product-02) | Product strategy & feature roadmap | Authenticate the proxy listener when bound to a non-loopback address | P0 | P1 | confirmed | S |
| [PRODUCT-04](findings/product-roadmap.md#product-04) | Product strategy & feature roadmap | Proxy bridge conformance suite (golden fixtures per protocol pair) | P0 | P1 | partially-confirmed | M |
| [PRODUCT-62](findings/product-roadmap.md#product-62) | Product strategy & feature roadmap | THEME — "Trust & governance" (vault, listener auth, signed catalog, disclosure, policy, audit, signing) | P0 | P1 | partially-confirmed | XL |
| [PRODUCT-63](findings/product-roadmap.md#product-63) | Product strategy & feature roadmap | THEME — "Projects everywhere" (per-directory binding, all apps in profiles/tray/universal/takeover, usage by project) | P1 | P1 | partially-confirmed | XL |
| [PRODUCT-65](findings/product-roadmap.md#product-65) | Product strategy & feature roadmap | THEME — "Reliability engineering" (bridge fixtures, e2e scenarios, beta ring, diagnostics) | P1 | P1 | partially-confirmed | L |
| [PRODUCT-66](findings/product-roadmap.md#product-66) | Product strategy & feature roadmap | Database backup restore skips the post-import live sync, settings reload and takeover reconciliation that import/WebDAV/S3 all run | P1 | P1 |  | S |
| [SEC-05](findings/security.md#sec-05) | Security & privacy | Secrets at rest are plaintext with default file permissions (SQLite DB, backups, exports, config dir); no Windows ACL hardening | P1 | P2 | partially-confirmed | M |
| [SEC-06](findings/security.md#sec-06) | Security & privacy | WebDAV/S3 sync uploads a plaintext SQL dump of all provider keys; plain-HTTP endpoints allowed | P1 | P2 | partially-confirmed | M |
| [SEC-07](findings/security.md#sec-07) | Security & privacy | Session "Resume" builds shell commands from transcript-controlled values and the Tauri command runs a free-form command string from the webview | P1 | P2 | partially-confirmed | S |
| [PROXY-01](findings/proxy-core.md#proxy-01) | Proxy core | Hyper raw-write fallback re-sends the request after it was already written to the upstream | P1 | P2 | confirmed | M |
| [PROXY-02](findings/proxy-core.md#proxy-02) | Proxy core | HalfOpen probe permit leaks when the client disconnects mid-request, leaving the provider stuck in HalfOpen | P1 | P2 | partially-confirmed | M |
| [PROXY-04](findings/proxy-core.md#proxy-04) | Proxy core | Blocking SQLite (std Mutex + sync I/O) on the async hot path, 6-10 times per request | P1 | P2 | partially-confirmed | L |
| [PROXY-05](findings/proxy-core.md#proxy-05) | Proxy core | Native-Anthropic path opens a new TCP+TLS connection per request; the raw-write mechanism is likely redundant | P1 | P2 | partially-confirmed | M |
| [PROXY-06](findings/proxy-core.md#proxy-06) | Proxy core | No end-to-end tests of the proxy against a mock upstream; `forward()` is untestable | P1 | P2 | partially-confirmed | L |
| [PROXY-07](findings/proxy-core.md#proxy-07) | Proxy core | Decompose `forwarder.rs`/`handlers.rs` into a staged request pipeline with pluggable transport | P1 | P2 | partially-confirmed | XL |
| [PROV-01](findings/proxy-providers.md#prov-01) | Proxy providers & format conversion | Claude←OpenAI-Chat stream converter silently swallows upstream error chunks and unparsable payloads | P1 | P2 | confirmed | S |
| [PROV-02](findings/proxy-providers.md#prov-02) | Proxy providers & format conversion | Streamed `tool_calls` without `index` are dropped (streaming.rs) or collapsed (streaming_codex_chat.rs) | P1 | P2 | partially-confirmed | S |
| [PROV-04](findings/proxy-providers.md#prov-04) | Proxy providers & format conversion | Claude→`openai_chat` ignores provider thinking parameters that the Codex→Chat path already models | P1 | P2 | partially-confirmed | M |
| [PROV-05](findings/proxy-providers.md#prov-05) | Proxy providers & format conversion | `gpt-5*` models through Chat Completions receive `max_tokens` and raw `temperature` | P1 | P2 | partially-confirmed | S |
| [PROV-19](findings/proxy-providers.md#prov-19) | Proxy providers & format conversion | Introduce a canonical message/stream IR with per-protocol codecs (replace 13 pairwise converters) | P1 | P2 | partially-confirmed | XL |
| [PROV-20](findings/proxy-providers.md#prov-20) | Proxy providers & format conversion | Build a recorded golden-fixture corpus and round-trip/property tests for the conversion layer | P1 | P2 | partially-confirmed | L |
| [SVC-03](findings/backend-services.md#svc-03) | Backend services | GitHub skill archive extraction skips `enclosed_name()` and has no entry/size caps (zip-slip / zip-bomb) | P1 | P2 | partially-confirmed | S |
| [SVC-04](findings/backend-services.md#svc-04) | Backend services | Sync uploads the full providers table (API keys, OAuth tokens) in plaintext | P1 | P2 | partially-confirmed | L |
| [SVC-05](findings/backend-services.md#svc-05) | Backend services | `futures::executor::block_on` inside synchronous Tauri commands blocks the main thread | P1 | P2 | partially-confirmed | M |
| [SVC-07](findings/backend-services.md#svc-07) | Backend services | Skill install/update write directly into the SSOT path; a mid-way failure leaves orphan or deleted skills | P1 | P2 | partially-confirmed | S |
| [SVC-18](findings/backend-services.md#svc-18) | Backend services | Introduce an `AppAdapter` trait and move per-app live/takeover/backfill logic out of the services | P1 | P2 | partially-confirmed | XL |
| [SVC-19](findings/backend-services.md#svc-19) | Backend services | Unify WebDAV and S3 behind a `SyncTransport` trait and a single auto-sync worker | P1 | P2 | partially-confirmed | L |
| [SVC-24](findings/backend-services.md#svc-24) | Backend services | No end-to-end tests for the sync protocol or auto-sync worker | P1 | P2 | partially-confirmed | M |
| [SVC-25](findings/backend-services.md#svc-25) | Backend services | No failure-injection tests for provider mutations, and several services have zero tests | P1 | P2 | partially-confirmed | M |
| [CMD-01](findings/commands-app-shell.md#cmd-01) | Commands / IPC / app shell | Deep-link URLs carrying API keys are logged in plaintext | P1 | P2 | confirmed | S |
| [CMD-03](findings/commands-app-shell.md#cmd-03) | Commands / IPC / app shell | Overly broad CSP and over-granted capabilities | P1 | P2 | partially-confirmed | S |
| [CMD-06](findings/commands-app-shell.md#cmd-06) | Commands / IPC / app shell | `save_settings` is a lost-update race on the process-global settings | P1 | P2 | partially-confirmed | M |
| [CMD-07](findings/commands-app-shell.md#cmd-07) | Commands / IPC / app shell | `futures::executor::block_on` on tokio locks reachable from async commands | P1 | P2 | partially-confirmed | L |
| [CMD-08](findings/commands-app-shell.md#cmd-08) | Commands / IPC / app shell | ~97 sync commands do file I/O, SQLite aggregation and process spawning on Tauri's sync-command thread | P1 | P2 | confirmed | L |
| [CMD-09](findings/commands-app-shell.md#cmd-09) | Commands / IPC / app shell | Startup does seconds of synchronous work before the window is shown | P1 | P2 | confirmed | L |
| [CMD-10](findings/commands-app-shell.md#cmd-10) | Commands / IPC / app shell | ~3,500 lines of business logic live in the command layer (and the tray calls commands) | P1 | P2 | partially-confirmed | XL |
| [CMD-12](findings/commands-app-shell.md#cmd-12) | Commands / IPC / app shell | Errors cross IPC as flattened strings; frontend substring-matches Chinese text | P1 | P2 | confirmed | L |
| [CMD-13](findings/commands-app-shell.md#cmd-13) | Commands / IPC / app shell | No typed IPC contract — 281 commands mirrored by hand in 24 TS files with inconsistent naming | P1 | P2 | partially-confirmed | XL |
| [CMD-31](findings/commands-app-shell.md#cmd-31) | Commands / IPC / app shell | Command/shell test coverage is thin and the test harness is awkward | P1 | P2 | partially-confirmed | L |
| [CFG-01](findings/config-adapters.md#cfg-01) | Per-app config adapters & MCP sync | Usage-script JS runtime has no CPU/memory limits and blocks the async runtime | P1 | P2 | confirmed | M |
| [CFG-02](findings/config-adapters.md#cfg-02) | Per-app config adapters & MCP sync | Gemini `settings.json` is silently replaced when it cannot be parsed | P1 | P2 | partially-confirmed | S |
| [CFG-03](findings/config-adapters.md#cfg-03) | Per-app config adapters & MCP sync | Claude/Codex/Gemini live files are overwritten with no backup and backfill is silently skipped when live is unreadable | P1 | P2 | confirmed | M |
| [CFG-04](findings/config-adapters.md#cfg-04) | Per-app config adapters & MCP sync | `atomic_write` is not crash-safe and not atomic on Windows; secret files inherit umask permissions | P1 | P2 | partially-confirmed | S |
| [CFG-05](findings/config-adapters.md#cfg-05) | Per-app config adapters & MCP sync | MCP specs from the UI and deeplinks bypass `validate_server_spec`; Windows `cmd /c` wrapping re-interprets shell metacharacters | P1 | P2 | partially-confirmed | S |
| [CFG-06](findings/config-adapters.md#cfg-06) | Per-app config adapters & MCP sync | Introduce an `AppAdapter` trait, a shared live-file layer, and a conformance test-suite (large refactor) | P1 | P2 | partially-confirmed | XL |
| [DB-03](findings/database.md#db-03) | Database / DAO / migrations | Single `Mutex<Connection>` without WAL/busy_timeout serialises the proxy write path, importers, UI reads and maintenance | P1 | P2 | partially-confirmed | XL |
| [DB-04](findings/database.md#db-04) | Database / DAO / migrations | Usage log inserts are one-row autocommit statements with 8 index B-trees to maintain | P1 | P2 | partially-confirmed | M |
| [DB-05](findings/database.md#db-05) | Database / DAO / migrations | Blocking SQLite work runs directly on tokio worker threads (no `spawn_blocking`) | P1 | P2 | confirmed | M |
| [DB-06](findings/database.md#db-06) | Database / DAO / migrations | Device-level tables/keys are synced wholesale (proxy_config, settings flags, session_log_sync) | P1 | P2 | partially-confirmed | M |
| [DB-07](findings/database.md#db-07) | Database / DAO / migrations | `create_tables_on_conn` is a second, untracked migration path with error-swallowing DDL and data fixups | P1 | P2 | partially-confirmed | L |
| [DB-09](findings/database.md#db-09) | Database / DAO / migrations | Maintenance and backfill hold the global lock for unbounded full-table work on every start | P1 | P2 | partially-confirmed | M |
| [DB-10](findings/database.md#db-10) | Database / DAO / migrations | Backup rotation, restore safety and pre-migration backups | P1 | P2 | partially-confirmed | M |
| [SESS-01](findings/sessions-deeplinks.md#sess-01) | Session manager, deep links & usage ingestion | Resume command / cwd are interpolated into shell strings without safe quoting; webview can run arbitrary commands | P1 | P2 | partially-confirmed | M |
| [SESS-03](findings/sessions-deeplinks.md#sess-03) | Session manager, deep links & usage ingestion | Tail-window reader drops `last_active_at`/`summary` when the final JSONL line is ≥16 KB, misordering busy sessions | P1 | P2 | partially-confirmed | S |
| [SESS-04](findings/sessions-deeplinks.md#sess-04) | Session manager, deep links & usage ingestion | Cold-start deep links are lost (no `get_current()`/pending buffer) and macOS handles the same link twice | P1 | P2 | confirmed | S |
| [SESS-06](findings/sessions-deeplinks.md#sess-06) | Session manager, deep links & usage ingestion | A real-looking Context7 API key is committed inside `deplink.html` examples | P1 | P2 | partially-confirmed | S |
| [FE-ARCH-01](findings/frontend-architecture.md#fe-arch-01) | Frontend architecture | `autoSyncConfirmed` setting is silently dropped by the backend (type drift between TS `Settings` and Rust `AppSettings`) | P1 | P2 | confirmed | S |
| [FE-ARCH-02](findings/frontend-architecture.md#fe-arch-02) | Frontend architecture | Replace the `App.tsx` god component with a router-driven shell and page-owned header actions | P1 | P2 | confirmed | XL |
| [FE-ARCH-03](findings/frontend-architecture.md#fe-arch-03) | Frontend architecture | Generate the Tauri command/event bindings from Rust (tauri-specta) instead of 291 hand-written `invoke` wrappers | P1 | P2 | confirmed | L |
| [FE-ARCH-04](findings/frontend-architecture.md#fe-arch-04) | Frontend architecture | Centralise TanStack Query keys and fix namespace collisions that over-invalidate third-party usage endpoints | P1 | P2 | partially-confirmed | M |
| [FE-ARCH-07](findings/frontend-architecture.md#fe-arch-07) | Frontend architecture | Linting and repo tooling are missing (ESLint referenced in CONTRIBUTING but absent, no knip, no bundle analysis, no Storybook) | P1 | P2 | confirmed | M |
| [FE-ARCH-10](findings/frontend-architecture.md#fe-arch-10) | Frontend architecture | App identity/capabilities are duplicated across 5+ hand-maintained lists and 45+ string comparisons; hardcoded label maps are already wrong for new apps | P1 | P2 | partially-confirmed | M |
| [FE-PROV-02](findings/frontend-providers.md#fe-prov-02) | Provider management UI | Edit dialog backfills live config after the form is already editable, resetting user input | P1 | P2 | partially-confirmed | M |
| [FE-PROV-03](findings/frontend-providers.md#fe-prov-03) | Provider management UI | Validation messages are hard-coded Chinese/English literals outside i18n | P1 | P2 | confirmed | S |
| [FE-PROV-04](findings/frontend-providers.md#fe-prov-04) | Provider management UI | Switching to the "Universal" tab in Add Provider unmounts the form and loses all input | P1 | P2 | confirmed | S |
| [FE-PROV-05](findings/frontend-providers.md#fe-prov-05) | Provider management UI | Hermes provider-key rename is accepted by the form but ignored on save | P1 | P2 | confirmed | S |
| [FE-UI-01](findings/frontend-features.md#fe-ui-01) | Feature UI (settings, usage, proxy, skills, MCP) | General-tab autosave silently persists unsaved Advanced-tab directory overrides (and skips their side effects) | P1 | P2 | partially-confirmed | M |
| [FE-UI-03](findings/frontend-features.md#fe-ui-03) | Feature UI (settings, usage, proxy, skills, MCP) | Directory override inputs trim on every keystroke, so paths containing spaces cannot be typed | P1 | P2 | confirmed | S |
| [FE-UI-05](findings/frontend-features.md#fe-ui-05) | Feature UI (settings, usage, proxy, skills, MCP) | Dead components hide unfinished features (request drill-down, session import, circuit-breaker panel, repo dialog, prompt modal, proxy-config hook, provider limits) | P1 | P2 | partially-confirmed | M |
| [FE-UI-16](findings/frontend-features.md#fe-ui-16) | Feature UI (settings, usage, proxy, skills, MCP) | XL refactor — split `WebdavSyncSection` into a backend-agnostic cloud-sync panel | P1 | P2 | partially-confirmed | XL |
| [FE-UI-17](findings/frontend-features.md#fe-ui-17) | Feature UI (settings, usage, proxy, skills, MCP) | L refactor — consolidate the settings save pipeline (`useSettings` / `useSettingsForm` / `useDirectorySettings`) | P1 | P2 | partially-confirmed | L |
| [FE-UI-42](findings/frontend-features.md#fe-ui-42) | Feature UI (settings, usage, proxy, skills, MCP) | Test gaps — no component tests for proxy, usage charts/pricing, about, backups, log/rectifier, OpenClaw, workspace, prompts, Hermes; S3 branch untested | P1 | P2 | partially-confirmed | L |
| [PERF-03](findings/performance.md#perf-03) | Performance & resources | Usage logging executes 5-6 synchronous SQL statements per request on Tokio worker threads, with no batching, no statement cache and no writer task | P1 | P2 | partially-confirmed | M |
| [PERF-04](findings/performance.md#perf-04) | Performance & resources | Every request deep-clones the full JSON body 3-5 times and rebuilds the tree twice before serialization | P1 | P2 | partially-confirmed | M |
| [PERF-05](findings/performance.md#perf-05) | Performance & resources | 6-8 database/config lookups per request before the first upstream byte (per-app proxy config read twice, full providers table scanned and JSON-parsed) | P1 | P2 | confirmed | M |
| [PERF-07](findings/performance.md#perf-07) | Performance & resources | Streaming usage collector retains every Gemini/OpenAI chunk as a parsed `serde_json::Value` for the whole stream | P1 | P2 | partially-confirmed | S |
| [PERF-08](findings/performance.md#perf-08) | Performance & resources | Cross-source dedup `NOT EXISTS` correlated subquery is re-evaluated inside every dashboard aggregate | P1 | P2 | partially-confirmed | M |
| [PERF-10](findings/performance.md#perf-10) | Performance & resources | Session-usage sync (every 60 s) issues one SELECT per JSONL file, one lock+INSERT per message with no transaction, and a fuzzy pricing lookup per message | P1 | P2 | confirmed | M |
| [PERF-11](findings/performance.md#perf-11) | Performance & resources | Startup does all configuration imports, DB maintenance and the full tray build synchronously inside `setup` before the window is shown, with no timing | P1 | P2 | partially-confirmed | M |
| [PERF-12](findings/performance.md#perf-12) | Performance & resources | 100 of 291 Tauri commands are synchronous (`pub fn`), including all dashboard aggregates, provider listing and skill scanning, so they execute on the main thread | P1 | P2 | confirmed | S |
| [PERF-13](findings/performance.md#perf-13) | Performance & resources | Idle polling storm: five overlapping 2-5 s React Query pollers plus a 10 s providers refetch, each hitting the global DB mutex and reading proxy config four times | P1 | P2 | partially-confirmed | M |
| [PERF-14](findings/performance.md#perf-14) | Performance & resources | Usage dashboard refetches all six queries on every `usage-log-recorded` event (200 ms backend debounce only) in addition to 30 s polling | P1 | P2 | confirmed | S |
| [PERF-15](findings/performance.md#perf-15) | Performance & resources | No code splitting: one monolithic bundle with every page, CodeMirror, recharts, flexsearch, 11k lines of presets and 135 KB of inline SVG strings | P1 | P2 | partially-confirmed | M |
| [PERF-17](findings/performance.md#perf-17) | Performance & resources | Single-connection design means long maintenance windows (rollup, backfill, session sync, backup) block the proxy logger and all UI commands | P1 | P2 | partially-confirmed | L |
| [TEST-02](findings/testing.md#test-02) | Testing & QA | Proxy format conversion has no golden-file corpus or whole-object assertions | P1 | P2 | confirmed | L |
| [TEST-03](findings/testing.md#test-03) | Testing & QA | Tauri command layer (285 commands) is effectively untested | P1 | P2 | confirmed | L |
| [TEST-04](findings/testing.md#test-04) | Testing & QA | MSW handler set drifts from the real command registry (22% coverage, 5 phantom handlers) | P1 | P2 | confirmed | M |
| [TEST-05](findings/testing.md#test-05) | Testing & QA | No contract between TS API wrappers/types and Rust serde shapes | P1 | P2 | partially-confirmed | L |
| [TEST-06](findings/testing.md#test-06) | Testing & QA | No coverage measurement or gate on either side | P1 | P2 | confirmed | M |
| [TEST-08](findings/testing.md#test-08) | Testing & QA | Integration tests share one process-wide HOME, a poisonable global mutex and a file-backed DB | P1 | P2 | partially-confirmed | M |
| [TEST-33](findings/testing.md#test-33) | Testing & QA | Prioritized roadmap to a trustworthy pyramid | P1 | P2 | partially-confirmed | XL |
| [CI-01](findings/ci-build-release.md#ci-01) | CI/CD, build, release & dependencies | Windows installers and portable EXE are not code-signed | P1 | P2 | confirmed | M |
| [CI-02](findings/ci-build-release.md#ci-02) | CI/CD, build, release & dependencies | Updater endpoint points at `/releases/latest/` while every release is published as a prerelease | P1 | P2 | confirmed | S |
| [CI-03](findings/ci-build-release.md#ci-03) | CI/CD, build, release & dependencies | Missing updater signatures only warn on Windows x64 and Linux, so a platform can silently drop out of `latest.json` | P1 | P2 | partially-confirmed | S |
| [CI-04](findings/ci-build-release.md#ci-04) | CI/CD, build, release & dependencies | No validation that the Tauri updater can actually consume what the release produced (`latest.json` + minisign verification) | P1 | P2 | partially-confirmed | S |
| [CI-05](findings/ci-build-release.md#ci-05) | CI/CD, build, release & dependencies | Secrets are interpolated into shell text and the updater private key is exported to `GITHUB_ENV` for all later steps | P1 | P2 | partially-confirmed | S |
| [CI-06](findings/ci-build-release.md#ci-06) | CI/CD, build, release & dependencies | Version is bumped by hand in four files plus CHANGELOG with no single source and no tag-vs-version guard | P1 | P2 | confirmed | S |
| [CI-07](findings/ci-build-release.md#ci-07) | CI/CD, build, release & dependencies | No release dry-run, and one platform failure blocks the whole release with no way to rehearse | P1 | P2 | partially-confirmed | M |
| [CI-08](findings/ci-build-release.md#ci-08) | CI/CD, build, release & dependencies | No vulnerability, license or unmaintained-crate scanning in CI | P1 | P2 | partially-confirmed | S |
| [CI-09](findings/ci-build-release.md#ci-09) | CI/CD, build, release & dependencies | Toolchain pins disagree across rust-toolchain.toml, Cargo.toml, CI, docs and package.json (Rust and Node/pnpm) | P1 | P2 | partially-confirmed | S |
| [CI-10](findings/ci-build-release.md#ci-10) | CI/CD, build, release & dependencies | CI never builds the frontend bundle or a Tauri bundle; the backend job runs against an empty `dist` placeholder | P1 | P2 | confirmed | M |
| [CI-11](findings/ci-build-release.md#ci-11) | CI/CD, build, release & dependencies | CONTRIBUTING documents `pnpm lint`/ESLint that do not exist; there is no JavaScript/TypeScript linter at all | P1 | P2 | partially-confirmed | M |
| [CI-37](findings/ci-build-release.md#ci-37) | CI/CD, build, release & dependencies | Ambitious — end-to-end release-engineering overhaul (release-please/git-cliff driven, dry-run, signed/attested, multi-channel) | P1 | P2 | partially-confirmed | XL |
| [I18N-01](findings/i18n.md#i18n-01) | Internationalization | Ten keys used in code are missing from every locale and leak Chinese fallbacks into non-Chinese UIs | P1 | P2 | confirmed | S |
| [I18N-02](findings/i18n.md#i18n-02) | Internationalization | First-run language falls back to Simplified Chinese for every non-CJK, non-English OS locale (frontend, tray and backend) | P1 | P2 | partially-confirmed | S |
| [I18N-04](findings/i18n.md#i18n-04) | Internationalization | zh-TW is missing 30 keys (tool-lifecycle section) and carries 2 orphan keys | P1 | P2 | confirmed | S |
| [I18N-05](findings/i18n.md#i18n-05) | Internationalization | No automated i18n integrity check in CI; the i18n PR label can never fire | P1 | P2 | confirmed | M |
| [DOCS-01](findings/docs-onboarding.md#docs-01) | Documentation & onboarding | The user manual is frozen at v3.16.0 and omits Grok Build, project profiles and S3 sync | P1 | P2 | partially-confirmed | L |
| [DOCS-02](findings/docs-onboarding.md#docs-02) | Documentation & onboarding | Release-notes/changelog hygiene: stale README link, no index, missing ja for 4 versions, 14 broken links, a missing 3.6.1 entry and a mis-dated 3.5.0 | P1 | P2 | partially-confirmed | S |
| [DOCS-04](findings/docs-onboarding.md#docs-04) | Documentation & onboarding | Deep-link protocol has no canonical spec; manual and generator lag the parser | P1 | P2 | partially-confirmed | M |
| [PRODUCT-03](findings/product-roadmap.md#product-03) | Product strategy & feature roadmap | Proactive provider health probing (documented but unimplemented) | P0 | P2 | confirmed | M |
| [PRODUCT-05](findings/product-roadmap.md#product-05) | Product strategy & feature roadmap | Finish budget limits & alerts (backend exists, UI/enforcement missing) | P1 | P2 | confirmed | M |
| [PRODUCT-06](findings/product-roadmap.md#product-06) | Product strategy & feature roadmap | System/tray notifications for failover, circuit open, budget, quota exhaustion and sync errors | P1 | P2 | confirmed | M |
| [PRODUCT-07](findings/product-roadmap.md#product-07) | Product strategy & feature roadmap | Remote, signed preset & sponsor catalog (decouple commercial changes from app releases) | P1 | P2 | partially-confirmed | L |
| [PRODUCT-08](findings/product-roadmap.md#product-08) | Product strategy & feature roadmap | In-app affiliate/sponsor disclosure and preset provenance | P1 | P2 | partially-confirmed | S |
| [PRODUCT-09](findings/product-roadmap.md#product-09) | Product strategy & feature roadmap | ToS-sensitive feature gate and README wording fix ("signature bypass") | P1 | P2 | partially-confirmed | S |
| [PRODUCT-10](findings/product-roadmap.md#product-10) | Product strategy & feature roadmap | Headless proxy daemon (proxy survives app quit / runs without window) | P1 | P2 | partially-confirmed | L |
| [PRODUCT-11](findings/product-roadmap.md#product-11) | Product strategy & feature roadmap | CLI companion (`ccs`) over the same SQLite store | P1 | P2 | partially-confirmed | L |
| [PRODUCT-12](findings/product-roadmap.md#product-12) | Product strategy & feature roadmap | Per-project (cwd / git repo) automatic profile binding | P1 | P2 | partially-confirmed | L |
| [PRODUCT-35](findings/product-roadmap.md#product-35) | Product strategy & feature roadmap | Windows Authenticode signing (SmartScreen) and SBOM/provenance for releases | P1 | P2 | confirmed | S |
| [PRODUCT-54](findings/product-roadmap.md#product-54) | Product strategy & feature roadmap | Sponsor preset governance policy (criteria, review cadence, removal, disclosure) as a public document | P1 | P2 | confirmed | S |
| [PRODUCT-61](findings/product-roadmap.md#product-61) | Product strategy & feature roadmap | THEME — "Proxy as a product" (daemon + CLI + proactive health + notifications + smart routing + web dashboard) | P1 | P2 | partially-confirmed | XL |
| [PROV-03](findings/proxy-providers.md#prov-03) | Proxy providers & format conversion | Gemini streaming text uses an ambiguous prefix heuristic that can drop or duplicate text | P1 | P3 | partially-confirmed | M |
| [SVC-06](findings/backend-services.md#svc-06) | Backend services | Best-effort takeover swallows live-write errors while the DB marks the app as taken over | P1 | P3 | partially-confirmed | S |
| [CMD-02](findings/commands-app-shell.md#cmd-02) | Commands / IPC / app shell | `copilot_get_token` / `copilot_get_token_for_account` hand raw bearer tokens to the webview | P1 | P3 | partially-confirmed | S |
| [CMD-04](findings/commands-app-shell.md#cmd-04) | Commands / IPC / app shell | Sync credentials (WebDAV password, S3 secret) persist in plaintext `settings.json` | P1 | P3 | partially-confirmed | M |
| [DB-08](findings/database.md#db-08) | Database / DAO / migrations | No migration ledger/checksums; `SCHEMA_VERSION` and sync `DB_COMPAT_VERSION` are independent hand-bumped constants | P1 | P3 | partially-confirmed | L |
| [FE-UI-02](findings/frontend-features.md#fe-ui-02) | Feature UI (settings, usage, proxy, skills, MCP) | `hermesConfigDir` is skipped by sanitisation, form normalisation and directory-change detection | P1 | P3 | partially-confirmed | S |
| [FE-UI-04](findings/frontend-features.md#fe-ui-04) | Feature UI (settings, usage, proxy, skills, MCP) | Failover/takeover UI covers different app sets in different places; label maps diverge | P1 | P3 | partially-confirmed | M |
| [PERF-06](findings/performance.md#perf-06) | Performance & resources | SSE passthrough parser allocates per line and memmoves the buffer per event | P1 | P3 | partially-confirmed | S |
| [PERF-09](findings/performance.md#perf-09) | Performance & resources | Provider limit checks wrap the indexed `created_at` column in `date()`/`strftime()` (non-sargable full scans) | P1 | P3 | partially-confirmed | S |
| [PERF-16](findings/performance.md#perf-16) | Performance & resources | Tray menu is rebuilt from scratch on every provider/profile/failover change, re-reading every provider table, blocking on async DB calls and parsing live config files from disk | P1 | P3 | partially-confirmed | M |
| [TEST-07](findings/testing.md#test-07) | Testing & QA | Zero tests for MCP sync services despite three recent MCP fixes | P1 | P3 | refuted | M |
| [I18N-06](findings/i18n.md#i18n-06) | Internationalization | CONTRIBUTING.md i18n and tooling instructions are wrong | P1 | P3 | confirmed | S |
| [DOCS-03](findings/docs-onboarding.md#docs-03) | Documentation & onboarding | README screenshots show a three-app UI that predates most shipped features | P1 | P3 | partially-confirmed | M |
| [DOCS-13](findings/docs-onboarding.md#docs-13) | Documentation & onboarding | First-run experience is a single text modal — add a guided setup wizard | P1 | P3 | partially-confirmed | L |

## Large refactors (L / XL)

41 proposals. These are the architectural bets for the next phase.

| ID | Area | Title | Priority | Effort |
|---|---|---|---|---|
| [PRODUCT-65](findings/product-roadmap.md#product-65) | Product strategy & feature roadmap | THEME — "Reliability engineering" (bridge fixtures, e2e scenarios, beta ring, diagnostics) | P1 | L |
| [CFG-09](findings/config-adapters.md#cfg-09) | Per-app config adapters & MCP sync | Build one format-preserving `LiveDoc` layer instead of five ad-hoc writers (large refactor) | P2 | L |
| [CFG-10](findings/config-adapters.md#cfg-10) | Per-app config adapters & MCP sync | Isolate `codex_history_migration.rs` behind a `Migration` registry with a retirement policy | P2 | L |
| [CMD-12](findings/commands-app-shell.md#cmd-12) | Commands / IPC / app shell | Errors cross IPC as flattened strings; frontend substring-matches Chinese text | P2 | L |
| [CMD-18](findings/commands-app-shell.md#cmd-18) | Commands / IPC / app shell | Per-app enumeration is hand-rolled in ~15 places; adding an app type touches all of them | P2 | L |
| [DB-15](findings/database.md#db-15) | Database / DAO / migrations | Raw SQL and direct `conn` locking spread across commands/services (layering violation) | P2 | L |
| [DB-17](findings/database.md#db-17) | Database / DAO / migrations | Column-per-app boolean flags force a schema migration per new app and already lag `AppType` | P2 | L |
| [DB-19](findings/database.md#db-19) | Database / DAO / migrations | Model pricing lives as ~900 lines of Rust in `schema.rs` and needs a release per price change | P2 | L |
| [FE-ARCH-03](findings/frontend-architecture.md#fe-arch-03) | Frontend architecture | Generate the Tauri command/event bindings from Rust (tauri-specta) instead of 291 hand-written `invoke` wrappers | P2 | L |
| [FE-ARCH-11](findings/frontend-architecture.md#fe-arch-11) | Frontend architecture | Error handling uses three incompatible protocols (string substring matching, JSON codes, raw messages) and toasts fire from every layer | P2 | L |
| [FE-ARCH-14](findings/frontend-architecture.md#fe-arch-14) | Frontend architecture | Forms use react-hook-form + zod in only 3 of ~20 forms, and the zod schema deliberately bypasses required-field validation | P2 | L |
| [FE-PROV-18](findings/frontend-providers.md#fe-prov-18) | Provider management UI | Three near-identical common-config stacks (hooks, modals, dark-mode observers) | P2 | L |
| [FE-PROV-22](findings/frontend-providers.md#fe-prov-22) | Provider management UI | Per-app config knowledge is scattered; `getApiKeyFromConfig` has a wrong Codex shape | P2 | L |
| [FE-UI-17](findings/frontend-features.md#fe-ui-17) | Feature UI (settings, usage, proxy, skills, MCP) | L refactor — consolidate the settings save pipeline (`useSettings` / `useSettingsForm` / `useDirectorySettings`) | P2 | L |
| [FE-UI-19](findings/frontend-features.md#fe-ui-19) | Feature UI (settings, usage, proxy, skills, MCP) | L refactor — decompose `AboutSection` (app update + tool lifecycle + WSL prefs + diagnostics + install commands) | P2 | L |
| [PERF-17](findings/performance.md#perf-17) | Performance & resources | Single-connection design means long maintenance windows (rollup, backfill, session sync, backup) block the proxy logger and all UI commands | P2 | L |
| [PRODUCT-44](findings/product-roadmap.md#product-44) | Product strategy & feature roadmap | Backend modularization: split `commands/misc.rs`, introduce `cc-switch-core` crate | P2 | L |
| [PROV-11](findings/proxy-providers.md#prov-11) | Proxy providers & format conversion | Gemini replay state (thought signatures, tool-call ids) lives only in a volatile in-memory shadow store | P2 | L |
| [PROV-18](findings/proxy-providers.md#prov-18) | Proxy providers & format conversion | Three SSE aggregators and two JSON-to-SSE expanders duplicate the streaming converters | P2 | L |
| [PROV-21](findings/proxy-providers.md#prov-21) | Proxy providers & format conversion | Replace string-dispatch `api_format` routing and the half-dead `ProviderAdapter` trait with a bridge registry | P2 | L |
| [SESS-10](findings/sessions-deeplinks.md#sess-10) | Session manager, deep links & usage ingestion | Architecture — deep-link v2 trust model: backend-held pending import, full-disclosure confirmation, signed/batch links, in-app share | P2 | L |
| [SVC-19](findings/backend-services.md#svc-19) | Backend services | Unify WebDAV and S3 behind a `SyncTransport` trait and a single auto-sync worker | P2 | L |
| [SVC-20](findings/backend-services.md#svc-20) | Backend services | Collapse the four session-usage importers into a `SessionSource` trait plus one `UsageIngest` writer | P2 | L |
| [SVC-21](findings/backend-services.md#svc-21) | Backend services | Split `proxy.rs` (7.2k lines) into takeover submodules and convert its `Result<_, String>` API to `AppError` | P2 | L |
| [TEST-09](findings/testing.md#test-09) | Testing & QA | Global mutable statics force 123 `#[serial]` tests and 130 env-var mutations | P2 | L |
| [CFG-06](findings/config-adapters.md#cfg-06) | Per-app config adapters & MCP sync | Introduce an `AppAdapter` trait, a shared live-file layer, and a conformance test-suite (large refactor) | P2 | XL |
| [CI-38](findings/ci-build-release.md#ci-38) | CI/CD, build, release & dependencies | Ambitious — split the 147k-LOC `cc-switch` crate into a Cargo workspace (core / proxy / cli / tauri shell) | P2 | XL |
| [CMD-10](findings/commands-app-shell.md#cmd-10) | Commands / IPC / app shell | ~3,500 lines of business logic live in the command layer (and the tray calls commands) | P2 | XL |
| [CMD-13](findings/commands-app-shell.md#cmd-13) | Commands / IPC / app shell | No typed IPC contract — 281 commands mirrored by hand in 24 TS files with inconsistent naming | P2 | XL |
| [DB-03](findings/database.md#db-03) | Database / DAO / migrations | Single `Mutex<Connection>` without WAL/busy_timeout serialises the proxy write path, importers, UI reads and maintenance | P2 | XL |
| [DB-20](findings/database.md#db-20) | Database / DAO / migrations | Whole-snapshot last-writer-wins sync with no row versioning or tombstones | P2 | XL |
| [FE-ARCH-02](findings/frontend-architecture.md#fe-arch-02) | Frontend architecture | Replace the `App.tsx` god component with a router-driven shell and page-owned header actions | P2 | XL |
| [FE-PROV-17](findings/frontend-providers.md#fe-prov-17) | Provider management UI | Split `ProviderFormFull` into a shell plus per-app form adapters | P2 | XL |
| [FE-UI-16](findings/frontend-features.md#fe-ui-16) | Feature UI (settings, usage, proxy, skills, MCP) | XL refactor — split `WebdavSyncSection` into a backend-agnostic cloud-sync panel | P2 | XL |
| [FE-UI-18](findings/frontend-features.md#fe-ui-18) | Feature UI (settings, usage, proxy, skills, MCP) | XL refactor — a shared `ManagedItemsPanel` for MCP, Skills and Prompts (and unify their data layer) | P2 | XL |
| [PRODUCT-64](findings/product-roadmap.md#product-64) | Product strategy & feature roadmap | THEME — "Platform SDK" (adapter trait, plugin surface for adapters/usage templates, new-app wave) | P2 | XL |
| [PROV-19](findings/proxy-providers.md#prov-19) | Proxy providers & format conversion | Introduce a canonical message/stream IR with per-protocol codecs (replace 13 pairwise converters) | P2 | XL |
| [PROXY-07](findings/proxy-core.md#proxy-07) | Proxy core | Decompose `forwarder.rs`/`handlers.rs` into a staged request pipeline with pluggable transport | P2 | XL |
| [SESS-09](findings/sessions-deeplinks.md#sess-09) | Session manager, deep links & usage ingestion | Architecture — backend-owned Session Index service (persistent index, incremental scan, file watching, typed sources) | P2 | XL |
| [SVC-18](findings/backend-services.md#svc-18) | Backend services | Introduce an `AppAdapter` trait and move per-app live/takeover/backfill logic out of the services | P2 | XL |
| [DB-08](findings/database.md#db-08) | Database / DAO / migrations | No migration ledger/checksums; `SCHEMA_VERSION` and sync `DB_COMPAT_VERSION` are independent hand-bumped constants | P3 | L |

## Quick wins (S effort, P0/P1)

10 items that a contributor can finish in under a day each.

| ID | Area | Title | Priority | Type |
|---|---|---|---|---|
| [CMD-40](findings/commands-app-shell.md#cmd-40) | Commands / IPC / app shell | Cold-start and lightweight-mode deep links are silently dropped (no get_current()/pending buffer) | P1 | bug |
| [FE-PROV-06](findings/frontend-providers.md#fe-prov-06) | Provider management UI | OMO custom agent/category rows vanish before a model is chosen | P1 | bug |
| [PERF-02](findings/performance.md#perf-02) | Performance & resources | SQLite opened with default journal/sync settings behind one global std Mutex (no WAL, synchronous=FULL, no busy_timeout, no cache tuning) | P1 | performance |
| [PRODUCT-02](findings/product-roadmap.md#product-02) | Product strategy & feature roadmap | Authenticate the proxy listener when bound to a non-loopback address | P1 | risk |
| [PRODUCT-66](findings/product-roadmap.md#product-66) | Product strategy & feature roadmap | Database backup restore skips the post-import live sync, settings reload and takeover reconciliation that import/WebDAV/S3 all run | P1 | bug |
| [SEC-02](findings/security.md#sec-02) | Security & privacy | Deep link `enabled=true` makes an attacker-supplied endpoint the *current* provider after one click; the flag is not shown in the confirmation dialog | P1 | security |
| [SEC-03](findings/security.md#sec-03) | Security & privacy | MCP deep link writes arbitrary stdio servers (command + args + env) into live tool configs; the dialog hides args/env and the "written immediately" warning is shown only when a parameter the backend ignores is set | P1 | security |
| [SEC-04](findings/security.md#sec-04) | Security & privacy | Raw deep-link URLs (including `apiKey`, `usageAccessToken`, `usageApiKey`) are written to the plaintext log file | P1 | security |
| [SESS-05](findings/sessions-deeplinks.md#sess-05) | Session manager, deep links & usage ingestion | Raw deep-link URL (including `apiKey`) is logged at info level and shipped in the `deeplink-error` payload | P1 | security |
| [SVC-08](findings/backend-services.md#svc-08) | Backend services | Gemini `settings.json` merge swallows parse errors and overwrites the file with provider-only content | P1 | bug |

## Area summaries

### Security & privacy

Full details: [findings/security.md](findings/security.md) (27 findings).

**Threat model.** Assets: (1) provider API keys and OAuth tokens for Claude Code / Codex / Gemini CLI / relays, stored in `~/.cc-switch/cc-switch.db`, `settings.json`, backups, sync payloads and the tools' live config files; (2) the user's prompts, source code and conversation transcripts that flow through the local proxy and the session manager; (3) the user's credit balance at each provider; (4) code-execution integrity of the machine (MCP stdio servers, terminals, skills). Attackers considered: a malicious web page or crafted `ccswitch://` link (no local access); a local unprivileged process or another local user (shared /tmp, log/db files); a LAN host when the proxy is bound to a non-loopback address (the UI accepts `0.0.0.0` with only a "recommended" hint); a malicious provider / relay (controls upstream responses and, via a deep link, can become the *current* provider that receives all prompts); a malicious skill repository / ZIP; a compromised update or CI channel.

**Overall posture.** The codebase shows real security intent in places: the Claude Desktop gateway has a random bearer token, the updater uses minisign with an embedded public key, backup/restore file names are validated, the local-ZIP skill path uses `enclosed_name()`, symlink targets are containment-checked, `open_external` is scheme-restricted, workspace file names are allow-listed, tool-lifecycle scripts validate tool names and WSL shells, `settings.json` is written with mode 0600, OAuth tokens are only sent to official endpoints, and the Claude GitHub workflow is gated to maintainers. The top risks are: **(a)** the main local proxy routes (`/v1/messages`, `/v1/responses`, `/v1beta/*` ...) accept requests from *anyone who can reach the socket* and inject the stored provider keys with no client authentication, no Origin/Host check and no CORS policy; **(b)** a single-click `ccswitch://` confirmation can make an attacker-controlled endpoint the *current* provider (`enabled=true`, not shown in the dialog) and can write arbitrary stdio MCP servers (args/env hidden in the dialog) into the live configs of Claude Code/Codex/Gemini; **(c)** raw deep-link URLs containing API keys are written to the plaintext log file at INFO level on macOS and at DEBUG elsewhere; **(d)** secrets are stored in plaintext SQLite/backups/sync dumps with default file permissions, sync uploads are unencrypted and WebDAV/S3 custom endpoints may be plain HTTP; **(e)** session "Resume" builds shell commands from transcript-controlled `sessionId`/`cwd` values and the Tauri command accepts a free-form command string from the webview; **(f)** provider keys are written to world-readable temp files on Linux when launching a provider terminal. No evidence of telemetry or key-harvesting relays was found; all 100+ preset endpoints are HTTPS, but ~104 preset/registration URLs carry affiliate/referral codes and the README/SECURITY.md contain no privacy or data-flow disclosure.

### Proxy core

Full details: [findings/proxy-core.md](findings/proxy-core.md) (48 findings).

The proxy core (server, forwarder, router, circuit breaker, response processor and the request-rectifier/optimizer helpers) is functionally rich and unusually well commented, with strong unit coverage of pure helpers (SSE parsing, content-encoding, URL rewriting, rectifiers, JSON canonicalization). The in-memory stores that grow with traffic (Gemini shadow, Codex chat history) are bounded, and the connection/usage guards are RAII-based. The main risks are structural and operational: `forwarder.rs` (4.8k lines, one ~1.1k-line `forward()` function) and `handlers.rs` (3.1k lines, five near-identical handler bodies, usage logging re-implemented ~6 times) carry all protocol, auth, header, timeout and transport decisions in one place, which is why several correctness gaps exist in the seams: a same-provider duplicate send in the hyper fallback, a HalfOpen permit that leaks when a client aborts (provider stuck until manual reset), a raw TCP+TLS connection per request with no connect timeout (an unreachable provider stalls failover for the full non-streaming timeout), and blocking SQLite calls on every request inside the async runtime. Security posture is adequate for the default loopback bind, but the UI allows `0.0.0.0` with no authentication on the main routes, and debug/error logs persist full prompts, response bodies, and upstream headers to the on-disk log file. Test coverage of the end-to-end failover/stream paths is effectively zero (no mock upstream, `forward()` requires a Tauri `AppHandle`). Suggested direction for the next phase: (1) fix the four P1 correctness/perf bugs with RAII guards and a single transport/timeout layer; (2) introduce an integration test harness with a mock upstream before (3) decomposing the forwarder into a staged request pipeline + pluggable transport + routing strategy, which also unlocks the requested features (sticky sessions, load balancing, retry budgets, metrics/tracing, flight recorder).

### Proxy providers & format conversion

Full details: [findings/proxy-providers.md](findings/proxy-providers.md) (43 findings).

The provider/format layer is large (~36k lines in scope, ~600 unit tests) and handles an impressive matrix of gateway quirks: unmarked-SSE sniffing, EOF-without-blank-line, `[1m]` model markers, DeepSeek/Kimi `reasoning_content` placeholders, Codex OAuth protocol constraints, Gemini missing-id synthesis. Its main structural risk is that four wire protocols (Anthropic Messages, OpenAI Chat, OpenAI Responses, Gemini) are bridged by roughly thirteen hand-written, mutually independent codecs (five request transforms, five streaming converters, three SSE aggregators), so every fix lands in one path and drifts from the others: the three recent commits (#5508 reasoning forward-attach, #5310 streamed tool-call identity, #4706 parameters type normalization) are complete on the Codex↔Chat/Responses paths but are not mirrored in the older Claude↔Chat path (`transform.rs`/`streaming.rs`), which still silently drops upstream error chunks, requires `tool_calls[].index`, ignores provider thinking parameters, and only normalizes schemas when `type` is absent. The Gemini bridge is the weakest leg: thinking is neither requested nor surfaced, streamed text uses an ambiguous prefix heuristic, replay state lives in a volatile in-memory shadow store, and unsupported `format` values can leak into the restricted schema channel. Usage accounting is internally consistent today, but the "does input include cache" rule is encoded as `app_type` string matches in three places plus implicit subtraction inside each transform. Auth managers are reasonable but store secrets in plaintext and have no HTTP-mocked tests for refresh/single-flight behaviour. Suggested direction for the next phase: (1) fix the P1 stream-correctness bugs, (2) introduce a canonical message/stream IR with per-protocol codecs and a bridge registry so each wire-format concern is implemented once, (3) back it with recorded golden fixtures plus round-trip property tests, then (4) close the provider matrix gaps (Gemini thinking, Azure/Bedrock/Vertex, Gemini OAuth refresh).

### Backend services

Full details: [findings/backend-services.md](findings/backend-services.md) (53 findings).

The services layer (~37k lines, 36 files) is where cc-switch's hardest invariant lives: the SQLite SSOT, the per-app "live" files, and the proxy-takeover backup slot must stay mutually consistent, and the code shows real care about this (common-config strip/merge symmetry in `provider/live.rs`, takeover ownership signals, OMO snapshot/rollback in `omo.rs`, hardened zip restore in `webdav_sync/archive.rs`, a transport-agnostic `sync_protocol.rs`). The main risks are (1) multi-step mutations that persist pointers before the live write succeeds, so a single failed write can later backfill the wrong provider's credentials into the DB; (2) cloud sync that is pure last-writer-wins with no precondition on the remote manifest, so two auto-syncing devices silently lose updates, and the uploaded `db.sql` carries every API key in plaintext; (3) `futures::executor::block_on` sprinkled through synchronous Tauri commands; and (4) one zip-extraction path that skipped the hardening the other two paths have. Structurally the layer has outgrown its shape: 178 `match app_type` arms are spread across eight files, `proxy.rs` is 7.2k lines returning `Result<_, String>`, the WebDAV/S3 pairs are near-verbatim copies, and the four session-usage importers each re-implement the same insert/dedup/pricing block. Tests are strong for Codex/Claude switch semantics and proxy takeover but absent for sync end-to-end, skill install/zip paths, universal providers, sort order, env manager, and failure injection. Suggested direction for the next phase: first close the P0/P1 consistency and security holes with small targeted changes plus failure-injection tests, then invest in three refactors that pay for themselves (an `AppAdapter` trait, a `SyncTransport` trait with conflict detection, and a shared usage-ingest pipeline), and standardize on `AppError` + `tracing` so partial failures become visible instead of being logged as `let _ =`.

### Commands / IPC / app shell

Full details: [findings/commands-app-shell.md](findings/commands-app-shell.md) (47 findings).

The command layer is large (285 `#[tauri::command]` functions across 34 files, 281 registered in `lib.rs`) and mostly thin, but a handful of files carry real business logic: `commands/misc.rs` alone is 5,414 lines (CLI tool lifecycle + cross-platform terminal launching), `commands/provider.rs` holds the per-template usage-query dispatcher, and `commands/usage.rs` runs raw SQL under `lock_conn!`. The "Commands → Services → DAO" rule is honoured for providers/MCP/skills/prompts/profiles but bypassed for usage, settings, proxy config, OpenClaw, Hermes and workspace commands. The app shell (`lib.rs` setup, `tray.rs`) is the riskiest area: it does seconds of synchronous DB/file work before the window is shown, mixes `futures::executor::block_on` with tokio locks in code paths reachable from async commands, and ~97 sync commands do file I/O / SQLite work on the thread Tauri gives sync commands. Security is mostly sound (path-traversal checks on backups and sessions, 0600 settings on Unix, redaction helper exists) but has concrete gaps: deep-link URLs containing API keys are logged at INFO, two commands return raw Copilot bearer tokens to the webview, the CSP allows `connect-src https: http:`, and sync credentials sit in plaintext JSON. Errors cross IPC as flattened strings, so the frontend substring-matches Chinese error text; the event and command contracts are hand-written in TypeScript and already drift (`proxy-flags-changed` is emitted but never listened to; `configLoadError` is listened to but never emitted). Suggested direction for the next phase: (1) a typed IPC contract (tauri-specta) with a structured `AppError` code enum, (2) move the misc/provider/proxy/failover orchestration into services and make `ProxyService`/`ProviderService` async-clean so every command can be `async` without `block_on`, (3) restructure startup into "migrate → show window → background import with progress events", and (4) close the logging/token/CSP security gaps.

### Per-app config adapters & MCP sync

Full details: [findings/config-adapters.md](findings/config-adapters.md) (48 findings).

The eight per-app adapters are functionally rich and the newest ones (OpenClaw with a round-trip JSON5 AST, Hermes with backups + write lock + duplicate-key healing, Codex with `toml_edit`) show the team already cares about preserving user files. The problem is that this care is uneven: Claude/Codex/Gemini live writes take no backup and are fully overwritten from the DB, OpenCode is read as JSON5 but written through a key-sorting `serde_json` writer (comments and order vanish), Gemini silently replaces an unparsable `settings.json` with `{}`-plus-provider, and `config::atomic_write` is neither crash-safe (no fsync) nor atomic on Windows (remove-then-rename). Cross-cutting logic is copy-pasted per app: `write_live_snapshot`/`read_live_settings`/backfill are 8-arm `match`es in `services/provider/live.rs`, the MCP sync module has three byte-identical `collect_enabled_servers` and six near-identical import loops, and `McpApps` is a struct of six booleans that every file enumerates by hand. Security-relevant gaps are concentrated in `usage_script.rs` (QuickJS runs with no CPU/memory limit on the async runtime, scripts evaluated twice, unbounded response bodies) and in MCP ingestion (UI/deeplink specs skip `validate_server_spec`; Windows `cmd /c` wrapping re-interprets shell metacharacters; custom MCP field values are logged at info level). The suggested direction is a single `AppAdapter` trait over a format-preserving `LiveDoc` layer with shared transactional writes/backups and a fixture-driven conformance suite that every adapter must pass; `codex_history_migration.rs` should move behind a `Migration` registry with a retirement policy, and `provider.rs` should be split into per-concern meta structs. With that foundation, the requested features (diff/preview before write, import-from-live for every app, schema validation, and new tools such as Zed/Qwen Code/Continue) become additive work instead of eight more copies.

### Database / DAO / migrations

Full details: [findings/database.md](findings/database.md) (44 findings).

The database layer is functional and has clearly had care put into it: migrations run under a SAVEPOINT with rollback, a pre-migration file backup is taken, SQL identifiers are validated before being interpolated, user-supplied filter values in `usage_stats.rs` are bound as parameters (no injection found), and the rollup/prune + cost-backfill pipeline has a solid unit-test suite. The main structural risk is the connection model: one `std::sync::Mutex<rusqlite::Connection>` opened with default rollback-journal mode (no WAL, no `busy_timeout`, no `synchronous` tuning) is shared by the proxy hot path (5 lock acquisitions and one fsync'd autocommit INSERT per proxied request), the session-log importers (one autocommit per row), the UI dashboard (which invalidates every usage query on each `usage-log-recorded` event), and daily maintenance (rollup/prune/VACUUM) that runs synchronously on tokio worker threads. Schema evolution is split between an ever-growing idempotent `create_tables_on_conn` (with error-swallowing `let _ = ALTER TABLE`) and a hand-rolled `match version` chain with no migration ledger; `SCHEMA_VERSION` and the sync `DB_COMPAT_VERSION` are independent hand-bumped constants. Security-wise, provider API keys, backups (`~/.cc-switch/backups/*.db`) and the cloud `db.sql` artifact are all plaintext, and the sync importer executes arbitrary SQL from the remote artifact after only a header-string check. Suggested direction for the next phase: (1) a dedicated writer thread + WAL + read connections, (2) a proper migration framework and single schema source of truth, (3) secrets off the plaintext path and a validated/encrypted sync format, (4) explicit device-vs-synced classification of tables/keys, and (5) moving raw SQL out of `commands/` and `services/` into the DAO.

### Session manager, deep links & usage ingestion

Full details: [findings/sessions-deeplinks.md](findings/sessions-deeplinks.md) (34 findings).

The Session Manager is a solid v1: seven providers are scanned in parallel (`session_manager/mod.rs:58-94`), per-provider parsers are unit-tested, deletes are root-validated and canonicalized (`mod.rs:145-195`), and the UI already has virtualization, grouping and batch delete. The main risks are (1) the resume path builds shell strings from untrusted transcript fields and accepts a free-form command from the webview (`terminal/mod.rs`, `commands/session_manager.rs`), (2) the head/tail metadata reader silently drops `last_active_at`/`summary` whenever the final JSONL line exceeds 16 KB, which misorders exactly the busiest sessions, and (3) there is no index: every refresh re-reads every session head/tail, Gemini/OpenCode scans parse whole files, and full transcripts cross IPC in one payload. The deep-link feature has reasonable parsing and URL validation but its confirmation model is thin: an MCP link writes arbitrary `command/args/env` into Claude/Codex live configs regardless of `enabled` while the dialog shows only a truncated command, prompt links can replace the live `CLAUDE.md` after a 500-char preview, cold-start links are dropped on Windows/Linux, and the raw URL (API key included) is logged at info level on one path. The usage-event pipeline works but emits every 200 ms during a backfill and runs blocking file scans on the async runtime. Suggested direction: a backend-owned Session Index service (persistent SQLite index + FTS + file watching + typed `SessionSource`), a typed cross-platform terminal launcher, and a deep-link v2 trust model (backend-held pending import, full-disclosure confirmation, signatures/bundles).

### Frontend architecture

Full details: [findings/frontend-architecture.md](findings/frontend-architecture.md) (36 findings).

The frontend has a reasonable three-layer skeleton (hand-written `src/lib/api/*` invoke wrappers -> TanStack Query hooks in `src/lib/query/*` and `src/hooks/*` -> components) with strict TypeScript, Prettier, Vitest+MSW and a useful `useTauriEvent` helper, and it has clearly been kept alive under heavy feature growth (8 managed apps, 268 Tauri commands, 2.5k i18n keys). The main structural risk is that the shell never got a router or a state model: `src/App.tsx` is a 1,673-line god component that owns navigation (a 14-value `View` union), per-view header actions wired through `useRef<any>` imperative handles, 12 `useState`, 6 `useRef`, 12 `useEffect` and 45 string comparisons on `activeApp`, and pages are still modelled as dialogs (`open`/`onOpenChange`). The second risk is the untyped Rust<->TS boundary: 291 hand-written `invoke` calls, duplicated wrappers, hooks that bypass the API layer, and type drift that already produces a real bug (`autoSyncConfirmed` is silently dropped by the Rust `AppSettings` struct). The query layer has no single key registry (77 literal `queryKey` arrays outside `lib/query`), two competing `useProxyStatus`/`useProxyConfig` implementations, 2-second polling where backend events already exist, and over-invalidation of third-party usage endpoints. Tooling is missing in ways that let drift accumulate: no ESLint config despite CONTRIBUTING referencing `pnpm lint`, no bundle analysis, all locales/presets/icons (~2.5 MB of source data) in the main chunk, no lazy routes. Suggested direction for the next phase: (1) introduce a router + app capability registry and dismantle `App.tsx`; (2) generate the command/event bindings from Rust with tauri-specta and delete the hand-written wrappers; (3) centralise query keys, invalidation and error handling; (4) land ESLint/knip/visualizer in CI and lazy-load pages, locales and presets.

### Provider management UI

Full details: [findings/frontend-providers.md](findings/frontend-providers.md) (55 findings).

The provider UI is feature-rich and clearly battle-hardened (sequence guards in the Codex common-config hook, SSOT protection for the Codex model catalog, Hermes URL validation, stable row keys for list editors), but it has grown by accretion into one 2.6k-line god component (`ProviderFormFull`) that mounts every app's hooks on every render and branches on `appId` in submit, preset handling, meta assembly and JSX. The most serious risks are state bugs rooted in that architecture: hooks receive non-reactive `form.getValues("settingsConfig")` snapshots so JSON-editor edits can be silently overwritten by the next field keystroke, the edit dialog backfills live config after the form is already editable, and the Add dialog's Tabs unmount the form (losing all input) when the user peeks at the "universal" tab. A second cluster is validation/i18n: zod and helper error strings are hard-coded Chinese or English literals, native `required` attributes short-circuit the app's own soft-validation, base-URL validation exists only for Hermes, and invalid Codex/Gemini editor state is silently replaced with stale defaults on save. A third cluster is UX/a11y: the full-screen panel has no dialog semantics or focus management, card actions are hover-only, TOML/.env editors use JavaScript highlighting, and cards for OpenCode/OpenClaw/Hermes/GrokBuild show "not configured" because endpoint extraction only knows Claude/Codex/Gemini shapes. Test coverage is thin where it matters: the dialogs are tested with the form mocked out, ProviderCard is covered only by a source-string test, and no test exercises a real `ProviderForm` submit for any app. Suggested direction: (1) fix the four data-loss bugs first (FE-PROV-01/02/04/05/06), (2) land the per-app "form adapter + config codec" architecture (FE-PROV-17/19/22) so new apps stop adding branches to the shell, (3) then build the high-value features on that base (health panel, command palette, groups/bulk ops, key hygiene).

### Feature UI (settings, usage, proxy, skills, MCP)

Full details: [findings/frontend-features.md](findings/frontend-features.md) (47 findings).

The non-provider feature UI is broad and mostly functional, with some genuinely careful work (password-preservation logic in WebdavSyncSection, the stale-while-revalidate tool cache in AboutSection, optimistic failover toggles, the live-range date picker). The main structural risk is size and duplication: `WebdavSyncSection.tsx` (1,867 lines) is two hand-mirrored state machines, `AboutSection.tsx` (1,272 lines) mixes five concerns, `useSettings`/`useSettingsForm` duplicate the save pipeline and the directory normalisation, and the MCP/Skills/Prompts panels re-implement the same "count bar + list + per-app toggles + confirm + form" shell three times with three different state strategies. There is a meaningful amount of dead code that hides unfinished features (`RequestDetailPanel`, `DataSourceBar`, `CircuitBreakerConfigPanel`, `RepoManager`, `PromptFormModal`, `hooks/useProxyConfig`, `useProviderLimits`) — the usage drill-down, session import and budgets are half-built but unreachable. Concrete bugs found: General-tab autosave silently persists unsaved Advanced-tab directory overrides without the save side-effects; `hermesConfigDir` is skipped by sanitisation and change detection; directory inputs trim on every keystroke so paths with spaces cannot be typed; failover settings only expose three apps while takeover exposes four (and the backend eight); zh-TW is missing 30 keys. Accessibility is uneven (hover-only action bars, icon buttons without names, hand-rolled modals without focus traps, no reduced-motion handling, light-only skeleton/badge colours). Test coverage is concentrated on settings/import/export and MCP form; proxy, usage charts, pricing, OpenClaw, workspace, prompts and the whole S3 branch have no component tests. Suggested direction: (1) fix the settings save-pipeline bugs and consolidate it into one table-driven module, (2) delete or wire the dead components, (3) land the three large refactors (cloud-sync panel, managed-items panel, AboutSection/tool lifecycle), (4) adopt zod + react-hook-form for the numeric config panels, (5) add an a11y/i18n lint pass, then (6) ship the missing features (usage export/budgets, request inspector, MCP server test, skill install progress).

### Performance & resources

Full details: [findings/performance.md](findings/performance.md) (40 findings).

The codebase (147.7k lines of Rust, 81.3k lines of TS/TSX, 750 crates, v3.17.0) has no performance instrumentation at all: no `benches/`, no criterion, no load-test harness, no startup timing spans, no bundle report, and CI (`.github/workflows/ci.yml`) only runs typecheck/format/unit tests. The biggest hot spots are in the proxy request path and the database layer: every Claude-format upstream request opens a brand-new TCP+TLS connection (`src-tauri/src/proxy/hyper_client.rs:373-432`), every request deep-clones the full JSON body 3-5 times and rebuilds the tree twice (`forwarder.rs:451-463, 1160-1169, 1527-1535`), and every request performs 6-8 SQLite statements under one global `std::sync::Mutex<Connection>` with no WAL, no `synchronous=NORMAL`, no statement cache and no transaction batching (`database/mod.rs:79-81, 109-116`; `proxy/usage/logger.rs:51-124, 220-312`). That same mutex is also held for long windows by the 60-second session-log sync, the startup rollup/backfill, and UI dashboard aggregates that run on the main thread as synchronous Tauri commands (`commands/usage.rs:12-116`), so the proxy's logger, the tray rebuild and the UI all serialize on it. Quick wins (hours each): enable WAL + `synchronous=NORMAL` + `busy_timeout`, add a `prepare_cached` pass, fold the SSE usage collector into an accumulator instead of retaining every chunk as a `serde_json::Value`, make the heavy commands `async`, lazy-load the four locale files (660 KB) and the 11k-line preset modules, and debounce the usage dashboard invalidation. Larger refactors worth planning: a dedicated usage-ingestion writer task with batching plus a WAL reader pool, a pooled keep-alive transport for the header-case-preserving path, an mtime-indexed session catalogue in SQLite, and route-level code splitting. Each finding below includes a measurement so the next phase can land with numbers; PERF-30 consolidates the measurement plan (criterion transforms/SSE benches, oha/k6 proxy load harness, startup spans, rollup-plugin-visualizer, `EXPLAIN QUERY PLAN` checks).

### Testing & QA

Full details: [findings/testing.md](findings/testing.md) (33 findings).

Frontend: 72 vitest files (68 under `tests/`: components 30, config 10, hooks 18, integration 2, lib 3, utils 5; plus 4 colocated `src/**/*.test.ts`) hold 462 tests and 12,966 LOC against 80,897 LOC of TS/TSX source (test:source ratio 0.16); whole component directories have zero tests (`src/components/proxy` 2,222 LOC, `universal` 1,173, `workspace` 812, `prompts` 608, `profiles` 484, `env` 275, `deeplink` 184, `hermes` 179), 10 of 26 hooks are never imported by a test, and the largest components never rendered by any test are `ProviderForm.tsx` (2,578 LOC; only its exported `normalizeCodexCatalogModelsForSave` helper is unit-tested), `CodexFormFields.tsx` (1,027), `ProxyPanel.tsx` (755) and `DeepLinkImportDialog.tsx` (730). Backend: `rg -c '#\[test\]|#\[tokio::test'` counts 2,060 inline test functions in 133 of 213 `.rs` files (~50.3k LOC of `mod tests` vs ~97.4k production LOC) plus 114 integration tests in 11 files under `src-tauri/tests/` (7,771 LOC), so the unit layer is dense (proxy 426 tests, proxy/providers 585, services 313, root modules 318) but 80 files have no tests at all, concentrated in the command layer (26 of 34 `commands/*.rs`, 285 `#[tauri::command]`s of which 192 take `State<>`/`AppHandle`, only 5 `*_test_hook` seams), the DAO layer (8 of 13 `database/dao/*.rs`), MCP sync (`mcp/codex.rs` 678 LOC, `services/mcp.rs` 546), the proxy server/failover plumbing (`proxy/server.rs` 405, `handler_config.rs` 228, `error.rs` 206, `failover_switch.rs` 135, `commands/proxy.rs` 455, `commands/failover.rs` 182) and `services/balance.rs` (454). Running the suite locally: `pnpm install --frozen-lockfile` succeeded (6.6 s), `pnpm typecheck` passed in 24 s, `pnpm test:unit` passed 72/72 files and 462/462 tests in 47.2 s (environment setup alone 39 s, slowest file `SessionManagerPage.test.tsx` 8.4 s) with 0 skipped tests, 0 act() warnings, but 18 `[MSW] Warning: intercepted a request without a matching request handler` (`get_installed_skills` x14, `list_profiles` x4), 4 `[App] Failed to sync window maximized state TypeError` from an unmocked `@tauri-apps/api/window`, 1 React unknown-prop warning, and 9 test files that fail `prettier --check` because `format:check` only scans `src/**`. The IPC mock layer (`tests/msw/`) is a faithful-in-spirit but tiny simulation: 58 handlers cover 58 of the 268 commands the frontend invokes (22%) and 285 commands registered in `lib.rs`, 5 handlers target commands that do not exist anywhere, there is no generated TS binding or contract check, and 15 of 72 test files bypass MSW by `vi.mock("@/lib/api")` wholesale. Backend tests are serialized through a process-global `test_mutex()` that is `.expect()`ed 109 times (a single panicking test poisons every later test in that binary), 123 `#[serial]` attributes and 130 `env::set_var` call sites compensate for global statics (`SETTINGS_STORE`, `APP_CONFIG_DIR_OVERRIDE`, `GLOBAL_CLIENT`), and there is no clock/uuid injection (64 `Utc::now()`, 14 `Uuid::new_v4()`), no proptest/insta/wiremock/cargo-fuzz, no `tauri::test` MockRuntime, no end-to-end proxy test (handlers.rs tests are pure-function tests; `proxy/server.rs` is never started in a test), no coverage provider installed, no E2E/UI harness, and CI runs `vitest` once on Ubuntu and `cargo test` on three OSes with no coverage gate, retries or JUnit output. Of the 50 commits available in this shallow clone, 26 are `fix`/`revert` and 11 of those sit in proxy format conversion / cache / usage accounting; 17 of 20 sampled fixes did add inline regression tests, so the missing piece is not discipline but infrastructure: golden corpora, property tests, an in-process proxy harness, command-layer tests and a contract between the TS API wrappers and the Rust command registry.

### CI/CD, build, release & dependencies

Full details: [findings/ci-build-release.md](findings/ci-build-release.md) (38 findings).

The pipeline already does several hard things well: a five-target release matrix (Windows x64/ARM64, Linux x64/ARM64, macOS universal) with Apple signing + notarization + stapling + verification, Tauri updater artifacts with minisign signatures, a scoped `release` environment, CODEOWNERS-gated `main`, Dependabot across npm/cargo/actions, and a backend CI matrix on three OSes. The main risks are release-integrity and process gaps rather than missing features: Windows installers ship unsigned, the updater endpoint (`/releases/latest/`) can never see a release because every release is created as a prerelease, missing `.sig` files only warn on x64/Linux so a platform can silently drop out of `latest.json`, secrets are interpolated into shell text and the updater private key is exported to `GITHUB_ENV` for every later step, and nothing (no dry-run, no tag-vs-version guard, no `latest.json` validation) stands between a hand-made 4-file bump commit and users' auto-updaters. CI itself is thin: no ESLint (despite CONTRIBUTING documenting `pnpm lint`), no vulnerability/license scanning, no frontend or Tauri bundle build on PRs, no coverage, no `--locked`, and toolchain pins that disagree in three places (rust-toolchain.toml 1.95 vs `rust-version` 1.85 vs CI `@stable`; `.node-version` 22.12 vs CI Node 20 vs "Node 18+" in docs). Dependency hygiene is the biggest hidden build cost: the app's own manifest pins lag what Tauri 2.10 already resolves, producing 79 duplicated crate names and three compiled TLS stacks (OpenSSL via reqwest's default `native-tls`, `ring`, and `aws-lc-rs` via rustls defaults), while mobile-template leftovers (`staticlib`/`cdylib` crate types, Android/iOS icons) and `strip = "symbols"` (which defeats the custom panic-hook backtraces) remain. Suggested direction: (1) harden release integrity first (Windows signing, mandatory signatures, `latest.json` validation, secret scoping, attestations); (2) make the release reproducible and automatable (single version source, bump script, tag guard, `workflow_dispatch` dry-run, generated changelog/release body, package-manager publishing); (3) trim the dependency graph and crate config for build speed; (4) fix the documented-but-missing developer tooling (ESLint, scripts, devcontainer, editorconfig); and (5) as the ambitious moves, a full release-engineering overhaul and a Cargo workspace split of the 147k-LOC crate into core/proxy/cli/tauri-shell so unit tests no longer need GTK/WebKit to run.

### Internationalization

Full details: [findings/i18n.md](findings/i18n.md) (20 findings).

The four UI locales (`src/i18n/locales/{en,zh,zh-TW,ja}.json`) are structurally healthy: 2,583 flattened keys in en/zh/ja, identical key order, 0 placeholder mismatches, 0 whitespace defects, 0 duplicate keys inside a file; the only parity gap is zh-TW with 30 missing keys (all `settings.tool*` tool-lifecycle strings, now rendered in English via `fallbackLng: "en"`) plus 2 orphan keys. Cross-referencing 2,394 literal `t()` calls (1,838 distinct keys, 14 template-literal and 39 dynamic-expression calls marked unverifiable) found 10 keys used in code but absent from every locale — each falls back to a hard-coded Chinese `defaultValue`, so English/Japanese/Traditional-Chinese users see Simplified Chinese toasts (e.g. `usage.unpriced` → "未定价") — and ~444 en keys (17%) that no code path references (the whole `console.*` namespace, 53 `omo.*`, 45 `proxy.*`, 44 `mcp.*`). Beyond JSON, the i18n boundary is porous: 802 `defaultValue:` fallbacks of which 459 are Chinese plus 195 positional `t("key", "中文")` defaults, ~247 CJK string literals outside `t()` (zod/JSON validation messages), and a Rust backend whose user-facing errors are Chinese-only (525 `Err/format!` lines, 53 `#[error("…")]` attributes, 228 commands returning `Result<_, String>`), which the frontend then matches by Chinese substring (`src/utils/errorUtils.ts:45-110`); proxy HTTP error bodies sent to Claude Code/Codex are Chinese too. First-run language detection falls back to `zh` for any non-CJK/non-English OS locale on both the frontend (`src/i18n/index.ts:11,57`) and the tray (`src-tauri/src/tray.rs:80-83,90-94`), so a German or Korean user gets a Chinese UI even though a German README exists. Process-wise, CONTRIBUTING.md still names three languages and the wrong paths, `.github/labeler.yml` watches a non-existent `src/locales/**`, `pnpm lint` is referenced but not defined, and CI (`.github/workflows/ci.yml`) has no locale-parity, dead-key or hard-coded-string check. On documentation: all four READMEs share an identical 589-line skeleton (11 FAQ items, 14 feature bullets each) but link release notes for v3.16.1 while the app is 3.17.0, the trilingual user manual (3×26 files) is stamped v3.16.0/2026-05-29 and has zero mentions of Grok Build, project profiles or S3 sync, the deep-link manual/generator cover 5/3 apps while the parser accepts 7, the README screenshots show a 3-app UI, 4 old versions lack Japanese release notes, 14 relative links in v3.7–3.8 release notes are broken, the CHANGELOG lacks a 3.6.1 entry and has a 3.5.0 date out of sequence. Onboarding is a single first-run modal whose text names only three tools, with no setup wizard, no in-app link to the manual, no in-app "what's new", and undocumented keyboard shortcuts.

### Documentation & onboarding

Full details: [findings/docs-onboarding.md](findings/docs-onboarding.md) (23 findings).

The four UI locales (`src/i18n/locales/{en,zh,zh-TW,ja}.json`) are structurally healthy: 2,583 flattened keys in en/zh/ja, identical key order, 0 placeholder mismatches, 0 whitespace defects, 0 duplicate keys inside a file; the only parity gap is zh-TW with 30 missing keys (all `settings.tool*` tool-lifecycle strings, now rendered in English via `fallbackLng: "en"`) plus 2 orphan keys. Cross-referencing 2,394 literal `t()` calls (1,838 distinct keys, 14 template-literal and 39 dynamic-expression calls marked unverifiable) found 10 keys used in code but absent from every locale — each falls back to a hard-coded Chinese `defaultValue`, so English/Japanese/Traditional-Chinese users see Simplified Chinese toasts (e.g. `usage.unpriced` → "未定价") — and ~444 en keys (17%) that no code path references (the whole `console.*` namespace, 53 `omo.*`, 45 `proxy.*`, 44 `mcp.*`). Beyond JSON, the i18n boundary is porous: 802 `defaultValue:` fallbacks of which 459 are Chinese plus 195 positional `t("key", "中文")` defaults, ~247 CJK string literals outside `t()` (zod/JSON validation messages), and a Rust backend whose user-facing errors are Chinese-only (525 `Err/format!` lines, 53 `#[error("…")]` attributes, 228 commands returning `Result<_, String>`), which the frontend then matches by Chinese substring (`src/utils/errorUtils.ts:45-110`); proxy HTTP error bodies sent to Claude Code/Codex are Chinese too. First-run language detection falls back to `zh` for any non-CJK/non-English OS locale on both the frontend (`src/i18n/index.ts:11,57`) and the tray (`src-tauri/src/tray.rs:80-83,90-94`), so a German or Korean user gets a Chinese UI even though a German README exists. Process-wise, CONTRIBUTING.md still names three languages and the wrong paths, `.github/labeler.yml` watches a non-existent `src/locales/**`, `pnpm lint` is referenced but not defined, and CI (`.github/workflows/ci.yml`) has no locale-parity, dead-key or hard-coded-string check. On documentation: all four READMEs share an identical 589-line skeleton (11 FAQ items, 14 feature bullets each) but link release notes for v3.16.1 while the app is 3.17.0, the trilingual user manual (3×26 files) is stamped v3.16.0/2026-05-29 and has zero mentions of Grok Build, project profiles or S3 sync, the deep-link manual/generator cover 5/3 apps while the parser accepts 7, the README screenshots show a 3-app UI, 4 old versions lack Japanese release notes, 14 relative links in v3.7–3.8 release notes are broken, the CHANGELOG lacks a 3.6.1 entry and has a 3.5.0 date out of sequence. Onboarding is a single first-run modal whose text names only three tools, with no setup wizard, no in-app link to the manual, no in-app "what's new", and undocumented keyboard shortcuts.

### Product strategy & feature roadmap

Full details: [findings/product-roadmap.md](findings/product-roadmap.md) (73 findings).

CC Switch (v3.17.0, MIT, Tauri 2 + React 18, 100k+ GitHub stars per `docs/release-notes/v3.16.3-en.md`) has evolved from a "provider switcher" into an all-in-one control plane for eight AI coding tools: 267 Tauri commands across 34 command modules, a 30-module Rust proxy with four protocol bridges (Anthropic/Chat/Responses/Gemini), unified MCP/Prompts/Skills sync, usage/cost accounting from both proxy logs and CLI session logs, cloud sync (WebDAV/S3), OAuth centers (Copilot, Codex), managed CLI-tool lifecycle, and (since 3.17.0) "Projects" snapshots. Release velocity is very high: eight releases between 2026-05-16 (v3.15.0, 127 commits) and 2026-07-13 (v3.17.0, 69 commits), with ~40-50% of each release's entries being *fixes* concentrated in four hotspots: proxy format-conversion bridges (Codex Chat/Responses/Anthropic), usage accounting (cache tokens, subagents, real upstream model), Codex `config.toml`/MCP projection (file blanking, deleted servers returning), and quota/usage-script queries (Zhipu tiers, retries). That fix distribution is the clearest signal of user pain: the proxy and billing paths are where power users live and where regressions hurt. The biggest unmet needs per persona are: (a) solo relay users have no budget alerts (the `limitDailyUsd` meta exists in the backend but is never surfaced or enforced), no key rotation, and no system notifications; (b) proxy power users have no proactive health probing (the manual documents it, `proxy/health.rs` is a 7-line stub), no model-aware/smart routing, no headless proxy, no CLI, and takeover/tray/profiles only cover a subset of the eight apps; (c) team leads can share via deep links and WebDAV/S3 but with no encryption, policy, audit log, or signed remote catalog; (d) security-conscious users find API keys in plaintext in SQLite, SQL exports and cloud-sync bundles, a LAN-bindable proxy with no listener auth, a permissive CSP, and three ToS-sensitive reverse-proxy features; (e) the Chinese relay ecosystem (35 of ~70 Claude presets are paid partners, hardcoded in `src/config/*Presets.ts`) churns every release (LionCC removed, MiniMax demoted, SudoCode promoted/demoted/promoted), requiring app releases for commercial changes and offering no in-app affiliate disclosure; (f) contributors face a 112-file / 4.5k-line change to add one app (Grok Build commit `1c0ee0c`), a 5,400-line `commands/misc.rs`, no adapter trait, no e2e tests, and stale CONTRIBUTING paths. Recommended next-phase themes: (1) "Proxy as a product" — proactive health, notifications, smart routing, headless daemon + CLI; (2) "Trust & governance" — OS keychain vault, encrypted sync, listener auth, signed remote preset/sponsor catalog with disclosure, ToS gating; (3) "Projects everywhere" — per-directory auto-binding, all eight apps in profiles/tray/universal/takeover, usage attribution by project; (4) "Platform SDK" — adapter trait + plugin surface so the next wave of tools (Qwen Code, Kimi CLI, Goose, Aider, Zed) does not cost 4.5k lines each; (5) "Reliability engineering" — bridge conformance fixtures and e2e switch/takeover tests to stop the fix treadmill.

## How this backlog was produced

Sixteen independent reviews ran in parallel, one per area (proxy core, proxy providers, services, commands/app shell, config adapters, database, sessions/deep links, frontend architecture, provider UI, feature UI, performance, testing, CI/build/release, security, i18n/docs, product). Each review read the code in its area and wrote findings in a fixed schema (type, priority, effort, confidence, evidence, problem, proposal, acceptance). Every finding rated P0 or P1 was then handed to an independent verifier with instructions to refute it by re-reading the cited code and searching for mitigations; their verdicts and corrected priorities are recorded under **Verification**. Finally, one completeness critic per high-leverage area (proxy core, proxy providers, services, config adapters, database, commands, provider UI, product) read the code the first pass had not cited and added 86 findings, marked `added by completeness-critic pass`; these were not independently re-verified, and where a critic finding overlaps an earlier one the area file says so under **Possible overlap with**. The files under `findings/` are those reviews, normalized; this README is generated from them. Line numbers refer to commit `613fef7`.
