# CCS-130: [proxy] Split forward() into request_builder, HeaderPolicy, transport and validate, leaving forwarder.rs a facade

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-request-pipeline-stages` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p3-recovery-strategy`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
forward() is about 1,100 lines, and forwarder.rs is 4,769.

## Proposed change
- pipeline/request_builder.rs (BodyStages: ModelMapping, VendorNormalize, Encode, LocalOverrides, Canonicalize).
- headers.rs: a declarative HeaderPolicy instead of the saw_* flags, at forwarder.rs:1819-1983.
- transport.rs and validate.rs (2xx semantics, stream priming).

## Acceptance criteria / verification
- [x] forwarder.rs is 600 lines or fewer; no pipeline function exceeds 150 lines
- [x] Golden upstream_request.expected is byte-identical
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`

## Dependencies
p3-recovery-strategy

## Implementation notes

**Summary.** `forward()` (≈1,100 lines) and the per-attempt helpers are split into stage modules under `src-tauri/src/proxy/pipeline/`; `forwarder.rs` is now a 556-line facade (public types, `ActiveConnectionGuard`, the failover loop, the per-provider recovery loop and a ~140-line `forward()` that only orchestrates stages). Behaviour is unchanged.

- `pipeline/request_builder.rs` — `RouteFlags` plus `BodyStages` with the stages in order: **ModelMapping** (model mapping / Claude Desktop route, thinking normalization, Grok Build upstream model, Copilot normalization + live resolution, `[1m]` strip), **VendorNormalize** (Copilot optimizer, Claude API format resolution, Anthropic message normalization, preventive media downgrade), **Encode** (Codex→Chat, Codex→Anthropic, Claude→other formats), **Canonicalize** (private-field filter + key order) and **LocalOverrides** (provider body overrides, re-canonicalized on change). Also builds the `RoutePlan`, the Bedrock pre-send body, the streaming check and the cache trace log.
- `pipeline/headers.rs` — `HeaderPolicy`: a declarative, first-match rule table (`Matcher` → `HeaderAction::{Drop, Pass, Rewrite, Fill(Slot), PassAndClaim(Slot)}`) plus ordered `Slot` rules (fill content + append-if-missing). This replaces the `saw_*` flags. The file also holds anthropic-beta / custom UA computation, Copilot header tweaks, Codex OAuth session headers, local header overrides and the managed-account placeholder guard.
- `pipeline/transport.rs` — reqwest vs raw hyper selection and header/first-byte timeouts.
- `pipeline/validate.rs` — `ResponseValidator`: non-2xx error bodies (decompressed), 2xx semantics (Anthropic error envelope, Responses `failed`/`cancelled`), non-streaming buffering and stream priming.
- Supporting stages: `pipeline/endpoint.rs` (endpoint rewrite + URL), `pipeline/auth.rs` (Copilot / Codex OAuth tokens, Codex official auth check), `pipeline/copilot.rs` (dynamic endpoint, live models, vendor, optimizer), `pipeline/classify.rs` (error categorization + failure logs). Failure settlement (`settle_failure`) moved into `pipeline/failover.rs` next to `AttemptRecorder`.
- Tests moved along with the code into each module. Facade-level tests live in `forwarder/tests.rs`.

**Byte-identity evidence.**
- New golden `src-tauri/src/proxy/forwarder/wire_golden.rs` drives `forward()` against a local TCP capture upstream for six routes: native Claude raw write with original header casing, Claude→OpenAI Chat with UA/header/body overrides, GET without body, Codex→Anthropic impersonation with `[1m]`, Codex→Anthropic plain with a full-endpoint base, and Codex→Chat. It compares the full request line, header order/casing and body with `src-tauri/tests/fixtures/forwarder/upstream_request.expected`. That file was **recorded against the pre-refactor code**, and the refactored code matches it byte for byte. CRLF is written as LF, with a check that rejects any bare CR/LF, so the file survives git EOL normalization.
- `HeaderPolicy` was also checked against a verbatim copy of the old `saw_*` loop across 210,816 combinations (64 flag combinations × auth/beta/UA/host variants × 61 random client header lists, including duplicates and every special header). The temporary oracle test passed and was removed before commit.
- Existing protocol golden snapshots (`tests/fixtures/proxy/**/upstream_request.snap`) are unchanged.

**Acceptance.**
- forwarder.rs: 556 lines (≤ 600). Longest pipeline functions: `forward` 139, `forward_with_retry_inner` 110; everything else is under 100.
- `cargo test --test proxy_golden --test proxy_e2e`: 21 + 6 passed.

**Verification.**
- `cargo fmt --check` → clean
- `cargo clippy -- -D warnings` and `cargo clippy --tests --lib -- -D warnings` → clean
- `cargo test --lib` → 2269 passed, 0 failed (3 ignored, pre-existing)
- `cargo test --test proxy_golden --test proxy_e2e` → 21 passed / 6 passed
- `cargo test --no-fail-fast` (full suite) → 2434 passed, 0 failed

**Commits.** 6a13161 (refactor + wire golden), plus a follow-up commit with the EOL-safe golden rendering and this note.
