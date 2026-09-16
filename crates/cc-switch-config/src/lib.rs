//! Pure-Rust ports of the cc-switch frontend config-manipulation utilities,
//! shared by the Tauri backend and the Dioxus (`wasm32-unknown-unknown`)
//! frontend. No filesystem, async runtime or Tauri dependency.
//!
//! | Module | TypeScript origin |
//! |---|---|
//! | [`text`] | `src/utils/textNormalization.ts` |
//! | [`toml_text`] | `src/utils/tomlUtils.ts` (+ TOML parse / error helpers) |
//! | [`provider_config`] | `src/utils/providerConfigUtils.ts` |
//! | [`grok_build`] | `src/utils/grokBuildConfig.ts` |
//! | [`version`] | `src/lib/version.ts` |
//!
//! Function names are the `snake_case` spelling of the TypeScript names so the
//! port stays traceable, and behaviour matches the TypeScript byte for byte,
//! including its quirks (CRLF is not normalised, arrays are compared
//! index-wise, TOML mutators collapse blank-line runs, …).

#![forbid(unsafe_code)]

pub mod grok_build;
pub mod provider_config;
pub mod text;
pub mod toml_text;
pub mod version;

pub use grok_build::{
    build_grok_build_config, extract_grok_build_base_url, parse_grok_build_config,
    update_grok_build_config, validate_grok_build_config, GrokBuildConfigValues,
    GROK_BUILD_DEFAULT_API_BACKEND, GROK_BUILD_DEFAULT_CONTEXT_WINDOW, GROK_BUILD_DEFAULT_MODEL,
};
pub use provider_config::{
    apply_template_values, codex_api_format_from_wire_api, extract_codex_base_url,
    extract_codex_experimental_bearer_token, extract_codex_model_name, extract_codex_top_level_int,
    extract_codex_wire_api, get_api_key_from_config, get_codex_base_url, has_api_key_field,
    has_common_config_snippet, has_toml_common_config_snippet, is_codex_anthropic_wire_api,
    is_codex_chat_wire_api, is_codex_goal_mode_enabled, is_codex_remote_compaction_enabled,
    remove_codex_top_level_field, set_api_key_in_config, set_codex_base_url, set_codex_goal_mode,
    set_codex_model_name, set_codex_remote_compaction, set_codex_top_level_int, set_codex_wire_api,
    update_codex_experimental_bearer_token, update_common_config_snippet, validate_json_config,
    CodexApiFormat, CodexWireApi, SetApiKeyOptions, TemplateValueConfig, UpdateCommonConfigResult,
};
pub use text::{normalize_quotes, normalize_toml_text};
pub use toml_text::{
    extract_id_from_toml, mcp_server_to_toml, toml_to_mcp_server, validate_toml, McpServerSpec,
    TomlSyntaxError,
};
pub use version::{compare_versions, is_update_available};
