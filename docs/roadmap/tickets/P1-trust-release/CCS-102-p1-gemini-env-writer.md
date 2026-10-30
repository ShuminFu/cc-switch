# CCS-102: [gemini] Validate .env keys and values, strip quotes when parsing, and keep unknown lines

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:apps-gemini`

> **Ticket key:** `p1-gemini-env-writer` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-gemini-env-injection.
- serialize_env_file writes raw KEY=value, so a deeplink apiKey containing %0A injects lines.
- The parser keeps surrounding quotes.
- Every write drops comments and lines the user added by hand.

## Evidence
- src-tauri/src/gemini_config.rs:26-50, 126-140, 156-190, 204-216; deeplink/parser.rs:99; deeplink/provider.rs:432-445; live.rs:1638/1643

## Proposed change
- Reject values containing \r, \n or \0, and keys that do not match [A-Za-z_][A-Za-z0-9_]*.
- Strip one pair of matching quotes and handle `export `.
- Merge on write, keeping comment lines and unknown keys.
- Reject control characters in the deeplink apiKey and model.

## Acceptance criteria / verification
- [ ] Tests: a value "k\nEVIL=1" gives Err; a quoted base URL parses without quotes; a round-trip keeps comments
- [ ] Deeplink test: an apiKey with %0A for gemini is rejected
- [ ] P0 gemini writer snapshot updated intentionally
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml gemini_config deeplink`

## Dependencies
p0-config-writer-snapshots

## Implementation notes

**Summary**
- `gemini_config.rs`: keys must match `[A-Za-z_][A-Za-z0-9_]*` (ASCII only, both parsers and the writer). Values containing `\r`, `\n` or `\0` are rejected with a localized `AppError` before anything is written. Values with leading/trailing whitespace, `#` or a leading quote are written in single quotes (double quotes as fallback; a value that needs quoting and holds both quote kinds is rejected), so they read back unchanged.
- The lax and strict parsers strip a leading UTF-8 BOM, accept `export KEY=VALUE` and strip one pair of matching `"`/`'` quotes.
- `write_gemini_env_atomic` now merges into the existing `.env` (`merge_env_file`): comments, blank lines and unparseable lines stay as they are; keys in the map are updated in place; keys cc-switch manages (`GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GOOGLE_GEMINI_BASE_URL`, `GEMINI_BASE_URL`, `GEMINI_MODEL`) are removed when the map leaves them out (needed for the OAuth switch); other keys the user added by hand are kept; new keys are appended in sorted order. `serialize_env_file` was replaced by `merge_env_file` (a test-only helper keeps the old name).
- Deeplink: a new `validate_no_control_chars` rejects control characters in `apiKey` and `model`. It runs in the URL parser and again in `build_provider_from_request`, so values that come from an inline base64 config are checked as well.
- P0 snapshot `tests/fixtures/live/gemini/{lf,crlf,bom}.snap` updated intentionally: the user comment and `FUTURE_GEMINI_VAR` now survive the write. The order follows the file, and `GEMINI_MODEL` is appended.

**Files**: `src-tauri/src/gemini_config.rs`, `src-tauri/src/deeplink/{utils,parser,provider,tests}.rs`, `src-tauri/tests/fixtures/live/gemini/{lf,crlf,bom}.snap`

**Commits**: 6f6df47 (code + snapshots), plus the follow-up commit that records these notes.

**Verification**
- New tests: `test_serialize_env_file_rejects_line_injection` (`"k\nEVIL=1"` gives Err), `test_serialize_env_file_rejects_invalid_keys`, `test_parse_env_file_strips_quotes_and_export` (a quoted base URL parses without quotes), `test_merge_env_file_keeps_comments_and_unknown_keys` (a round-trip keeps comments and is idempotent), `test_env_value_quoting_round_trips`, `test_parse_gemini_deeplink_rejects_newline_in_api_key` (`apiKey=k%0AEVIL%3D1` is rejected), `test_build_gemini_provider_rejects_control_chars_from_config`.
- `cargo test --manifest-path src-tauri/Cargo.toml -- gemini_config deeplink`: 73 lib + 4 integration passed.
- `INSTA_UPDATE=always cargo test --test config_writers`: 17 passed; snapshot diff reviewed.
- `cargo fmt`: clean. `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --no-fail-fast` (full suite): 2454 passed, 0 failed (lib: 2288 passed, 2 ignored).

**Review**
- Keeping unknown keys on write leaked variables across providers: before a switch the live `.env` is backfilled into the current provider, so every valid key already lives in a provider config. A key kept by the writer then went into the next provider's `.env`, and the next backfill saved it into that provider in the DB (this could include another provider's secret). It also meant a custom variable deleted in the editor never left `.env`, and a rollback snapshot could not remove keys added by the failed write. The writer now treats the map as the full set of variables. Keys in the map are updated in place or appended. Every other valid key is removed. Comments, blank lines, unparseable lines and lines with invalid key names are kept. `MANAGED_ENV_KEYS` is gone. The P0 snapshots no longer keep `FUTURE_GEMINI_VAR`, but the user comment still survives.
- Configs saved by the old parser can hold values with their quotes (`"sk-abc"`). The new quoting wrote these as `'"sk-abc"'`, so Gemini CLI would read the key with the quotes and fail to authenticate. A value fully wrapped in one pair of matching quotes is now written as-is, and it reads back unquoted.
- New and changed tests: `test_merge_env_file_keeps_comments_and_drops_stale_keys`, `test_env_value_legacy_quoted_written_verbatim`, and `test_env_value_quoting_round_trips` (literal fully-quoted values replaced with unbalanced ones).
- Re-verified: `cargo fmt` and `cargo clippy --all-targets -- -D warnings` are clean. `cargo test --test config_writers` gives 17 passed. Full `cargo test --no-fail-fast` gives 2455 passed, 0 failed (lib: 2289 passed, 2 ignored).
