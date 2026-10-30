# CCS-215: [deps] Upgrade the direct zip dependency to a release that fixes RUSTSEC-2026-0367/0368/0369

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:deps`

> **Ticket key:** `p0-deps-zip-upgrade` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p0-deps-cargo-update-deny-baseline`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-018, CCS-001). Source tickets: [CCS-018](../P0-safety-net/CCS-018-p0-ci-cargo-deny.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
`src-tauri/Cargo.toml:73` has `zip = "2.2"` (resolves to 2.4.2), which is affected by RUSTSEC-2026-0367/0368/0369; the three IDs sit in the `deny.toml` ignore list. A second copy, zip 4.6.1, comes in through tauri-plugin-updater. Fixing it needs a semver-major bump and call-site changes, so it does not fit in the plain `cargo update` of CCS-214.

## Proposed change
- Bump `zip` to the first release that fixes the three advisories (check with `cargo deny` / the RustSec DB).
- Adapt the call sites: `src-tauri/src/services/skill.rs`, `services/webdav_sync/archive.rs`, `proxy/capture/export.rs`, `proxy/capture/tests.rs`, `commands/skill.rs`.
- If a tauri-plugin-updater update moves its copy to a fixed zip, take it. Otherwise keep the ignore entries but reword them to name only the transitive updater copy.
- `proxy/capture/*` is under `src-tauri/src/proxy/`, so the PR needs a fixture change or the `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` passes, including skill import/export, the WebDAV archive and capture export tests.
- [x] `cargo deny --manifest-path src-tauri/Cargo.toml check advisories` passes with the three zip ignores removed, or narrowed to the updater copy.
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.

## Dependencies
CCS-214 (`p0-deps-cargo-update-deny-baseline`).

## Implementation notes

**Summary.** The direct `zip` dependency moves from `2.2` (2.4.2) to `9.1` (9.1.3). 9.0.0 is the first release that fixes RUSTSEC-2026-0367/0368/0369. zip 9's MSRV is 1.88, which matches the crate's `rust-version`. Default features are kept, so archives that use bzip2, zstd, lzma or AES keep working. The only call-site break was `ZipFile::name()`, which now returns `ZipResult<Cow<str>>`:
- `services/skill.rs`: the two "skipping unsafe ZIP entry" warnings now log `String::from_utf8_lossy(file.name_raw())`.
- `proxy/capture/tests.rs` and `tests/proxy_capture.rs`: test helpers unwrap the name.
- `services/webdav_sync/archive.rs`, `proxy/capture/export.rs` and `commands/skill.rs` compile unchanged.

None of our code calls `ZipArchive::extract()`. Our extractors use `enclosed_name()`, write files with `File::create` and resolve symlinks under confinement. A new unix-only regression test, `extract_repo_archive_drops_setuid_setgid_and_sticky_bits`, pins down that SUID/SGID/sticky bits from an archive (mode `0o7755`) never reach an extracted file. Our extractor never copied these bits, so the test also passes against zip 2.x. It is a guard against future changes, not a reproduction of a bug.

**tauri-plugin-updater copy.** The lock has tauri-plugin-updater 2.10.0, which depends on zip 4.6.1, and only on Windows (`[target.'cfg(target_os = "windows")'.dependencies.zip]`), where it extracts a signature-verified update bundle. The three ignore entries in `src-tauri/deny.toml` stay, but are reworded to name only that transitive copy. (Corrected in review: 2.10.0 is not the latest 2.x; see Review below.)

**Side effects in `deny.toml`.**
- zip 9 brings in `libbz2-rs-sys` (license `bzip2-1.0.6`, a permissive BSD-style license). It is allowed with a per-crate `[licenses].exceptions` entry, not added to the global allow list.
- zip's zstd copy is now 0.14.3 / zstd-safe 8.2.0. The zstd ignores (RUSTSEC-2026-0391/0398/0403) now refer only to our direct `zstd = "0.13"` dependency, and their comment is updated to say so.

**Fixture policy (CCS-034).** Under `src-tauri/src/proxy/`, only the test helper in `proxy/capture/tests.rs` changes. There is no behaviour change, so the PR needs the `no-fixture-needed` label.

**Files:** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/deny.toml`, `src-tauri/src/services/skill.rs`, `src-tauri/src/proxy/capture/tests.rs`, `src-tauri/tests/proxy_capture.rs`.

**Commits:** e231824 (code); this ticket edit follows in a separate commit.

**Verification (Linux):**
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3098 passed, 0 failed across all test binaries. This includes the skill archive tests, the WebDAV archive tests and the capture export tests (`tests/proxy_capture.rs`, `proxy::capture::tests`).
- `cargo test --lib services::skill`: 12 passed, including the new SUID test.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- `cargo deny --manifest-path src-tauri/Cargo.toml check`: `advisories ok, bans ok, licenses ok, sources ok`.
- With the three zip ignores removed, `cargo deny check advisories` fails only on `zip v4.6.1`, the updater copy. This confirms the direct dependency is clean and that the remaining entries cover only the updater copy.

**Review.** Independent review found two gaps and fixed them in a follow-up commit:
- The SUID regression test built its fixture with `SimpleFileOptions::unix_permissions(0o7755)`, but zip 9 masks that argument to `0o777`. The archive never carried SUID/SGID/sticky bits, so the test checked nothing. The fixture now sets `external_attributes(0o107755 << 16)`, and a pre-assertion checks the raw archive bytes for that value. (`unix_mode()` cannot be used here: zip 9 clears the special bits when reading.)
- tauri-plugin-updater 2.10.0 is not the latest 2.x. 2.14.1 is out and moves its Windows zip dependency to `^9`, but it needs `tauri ^2.12` (we lock 2.10.3) and `rust-version` 1.90 (our MSRV is 1.88). Every 2.x release that fits our MSRV (up to 2.12.0) still uses zip 4. Bumping the MSRV and tauri is outside this ticket, so the ignores stay. The `deny.toml` comment now names 2.14.1 and that blocker; drop the ignores once that bump lands.
- Re-verified: `cargo test --no-fail-fast` 3098 passed / 0 failed (before the test fix); `cargo test --lib services::skill` 12 passed after it; clippy `--all-targets -D warnings` and fmt clean; `cargo deny check` ok.
