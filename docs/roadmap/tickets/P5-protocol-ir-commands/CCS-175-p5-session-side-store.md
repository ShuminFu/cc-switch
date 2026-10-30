# CCS-175: [proxy] Move the Gemini shadow, Codex chat history and reasoning bridge behind SessionSideStore

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-session-side-store` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-ir-codec-core`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
One SessionSideStore trait passed to the codecs. GeminiShadowStore, CodexChatHistoryStore and reasoning_bridge implement it.

## Acceptance criteria / verification
- [x] Golden fixtures that depend on the side stores (thought signatures, history restore) are unchanged
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-ir-codec-core

## Implementation notes

**Summary:**
- `proxy/codec/side_store.rs` (new) defines `SessionSideStore`:
  - `record(scope, &mut Record)` runs on the response side and `restore(scope, &mut Restore)` runs on the request side. Both return how many entries were recorded or restored, and 0 means the payload was not changed.
  - `Record` and `Restore` are associated types, so each store keeps its own protocol-private payload. The trait stays object-safe, for example `dyn SessionSideStore<Record = Value, Restore = Value>`.
  - `SideStoreScope { provider_id, session_id }` is the shared scope.
  - `SideStoreKind` (`gemini_shadow`, `codex_chat_history`, `reasoning_bridge`) uses the same names as the `RoutePlan.side_stores` flags.
  - Codecs that need side state take the store and the scope when they are built. The IR carries no protocol-private fields.
- Implementations:
  - `GeminiShadowStore`:
    - `Record = GeminiAssistantTurn`, `Restore = Vec<GeminiAssistantTurn>`. `restore` appends the session's turns, oldest first.
    - Both methods do nothing when there is no session ID, which matches the old behaviour.
  - `CodexChatHistoryStore`:
    - `Record = Value`: a Responses response.
    - `Restore = Value`: a Responses request whose missing function calls are restored.
    - It ignores the scope, because it is keyed by `previous_response_id` / `call_id` as before.
    - It moves from `tokio::sync::RwLock` to `std::sync::RwLock` with poison recovery, so its methods are now synchronous. No lock was held across an await.
  - `ReasoningBridge` (new unit struct in `reasoning_bridge.rs`):
    - It is stateless. `record` replaces a Responses `reasoning` item in place with the Anthropic thinking or redacted_thinking block that carries it, and `restore` turns it back.
    - It delegates to the existing free functions.
- Legacy paths now go through the trait:
  - `anthropic_to_gemini_with_shadow` restores the shadow turns.
  - `gemini_to_anthropic_with_shadow_and_hints` and the Gemini SSE converter record turns.
  - The Codex Responses→Chat request builder (`encode_codex_chat`) restores history.
  - The non-stream Codex handler and the SSE recorder call the now-synchronous inherent methods, which are the same code the trait delegates to.
  - `transform_responses.rs` / `streaming_responses.rs` keep calling the reasoning-bridge free functions, which `ReasoningBridge` wraps. They have no provider or session scope.

**Files:**
- `src-tauri/src/proxy/codec/side_store.rs` (new)
- `src-tauri/src/proxy/codec/side_store/tests.rs` (new)
- `src-tauri/src/proxy/codec/mod.rs`
- `src-tauri/src/proxy/providers/gemini_shadow.rs`
- `src-tauri/src/proxy/providers/codex_chat_history.rs`
- `src-tauri/src/proxy/providers/reasoning_bridge.rs`
- `src-tauri/src/proxy/providers/transform_gemini.rs`
- `src-tauri/src/proxy/providers/streaming_gemini.rs`
- `src-tauri/src/proxy/pipeline/request_builder.rs`
- `src-tauri/src/proxy/handlers.rs`

**Commits:** 8f5bd01 (code), plus the follow-up commit with this note.

**Tests:** `proxy::codec::side_store::tests` (7 tests). They cover:
- Gemini record and restore through an `Arc<dyn SessionSideStore>`
- provider and session isolation, with no record when there is no session
- a shadow recorded through the trait and replayed by the legacy encoder, restoring the `thoughtSignature` and the `functionResponse` name
- Codex history restore through a trait object, where record is read-only and an unrelated request is left unchanged
- the reasoning-bridge round trip across scopes, with unconvertible payloads left unchanged
- kind names matching the RoutePlan flags

The existing shadow and history tests in `transform_gemini`, `streaming_gemini` and `codex_chat_history` now run through the trait or the synchronous store. The Codex tests changed from `#[tokio::test]` to `#[test]`, except the SSE recorder test.

**Verification:**
- `cargo fmt --check`: clean
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test --lib -- side_store codex_chat_history gemini_shadow reasoning_bridge transform_gemini streaming_gemini`: 66 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 6 passed, with no fixture diffs (the `anthropic__gemini` thought-signature fixtures are unchanged)
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2766 passed, 0 failed
