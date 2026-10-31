//! Property tests for the hand-edited config parsers (TEST-20).
//!
//! Users edit `~/.codex/config.toml`, Grok Build's `config.toml` and
//! `opencode.json` by hand. Whatever bytes they contain, the parsers must
//! return `Ok`/`Err`/`None` (never panic inside a Tauri command), edits must be
//! idempotent, and unrelated keys and comments must survive an edit.
//!
//! Case counts default to 64 and follow `PROPTEST_CASES` (nightly uses 2000).

use proptest::prelude::*;

fn config(default_cases: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default_cases);
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Text that is often TOML-shaped but frequently broken.
fn tomlish() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        "[a-z_]{1,10} = \"[ -~&&[^\"\\\\]]{0,20}\"",
        "[a-z_]{1,10} = [0-9]{1,5}",
        "\\[[a-z_.]{1,20}\\]",
        "\\[\\[[a-z_]{1,10}\\]\\]",
        "# [ -~]{0,30}",
        "[ -~]{0,30}",
        Just("model_provider = \"custom\"".to_string()),
        Just("[model_providers.custom]".to_string()),
        Just("base_url = \"https://example.com/v1\"".to_string()),
        Just("[models]".to_string()),
        Just("default = \"m\"".to_string()),
        Just("[model.\"m\"]".to_string()),
        Just(String::new()),
    ];
    prop::collection::vec(line, 0..14).prop_map(|lines| lines.join("\n"))
}

/// A valid Codex config with a random provider, model and comment lines.
fn codex_config() -> impl Strategy<Value = (String, String, String)> {
    (
        "[a-z][a-z0-9_]{0,10}",
        "[a-z][a-z0-9.\\-]{0,12}",
        prop::collection::vec("[a-zA-Z0-9 ]{0,20}", 0..4),
    )
        .prop_map(|(provider, model, comments)| {
            let mut text = String::new();
            for c in &comments {
                text.push_str(&format!("# {c}\n"));
            }
            text.push_str(&format!(
                "model_provider = \"{provider}\"\nmodel = \"{model}\"\n\n[model_providers.{provider}]\nname = \"{provider}\"\nbase_url = \"https://old.example.com/v1\"\nwire_api = \"responses\"\n\n[profiles.keep]\nmodel = \"untouched\"\n"
            ));
            (text, provider, model)
        })
}

fn parse(text: &str) -> toml_edit::DocumentMut {
    text.parse().expect("valid TOML")
}

proptest! {
    #![proptest_config(config(64))]

    #[test]
    fn codex_parsers_never_panic_on_arbitrary_text(text in tomlish()) {
        let _ = crate::codex_config::validate_config_toml(&text);
        let _ = crate::codex_config::extract_codex_base_url(&text);
        let _ = crate::codex_config::extract_codex_experimental_bearer_token(&text);
        let _ = crate::codex_config::codex_config_has_official_proxy_route(&text);
        let _ = crate::codex_config::remove_codex_official_proxy_route(&text);
        let _ = crate::codex_config::inject_codex_unified_session_bucket(&text);
        let _ = crate::codex_config::strip_codex_unified_session_bucket(&text);
        let _ = crate::codex_config::update_codex_toml_field(&text, "base_url", "https://x");
        let _ = crate::codex_config::remove_codex_toml_base_url_if(&text, |_| true);
    }

    #[test]
    fn grok_parsers_never_panic_on_arbitrary_text(text in tomlish()) {
        let _ = crate::grok_config::validate_config_toml(&text);
        let _ = crate::grok_config::extract_model_config(&text);
        let _ = crate::grok_config::extract_credentials(&text);
        let _ = crate::grok_config::extract_base_url(&text);
        let _ = crate::grok_config::update_api_key(&text, "sk-new");
        let _ = crate::grok_config::has_proxy_placeholder(&text, "PROXY");
    }

    /// Updating a field is idempotent and the value reads back.
    #[test]
    fn codex_field_update_is_idempotent_and_reads_back(
        (text, provider, _model) in codex_config(),
        url in "https://[a-z]{1,10}\\.example\\.com/v[0-9]",
    ) {
        let once = crate::codex_config::update_codex_toml_field(&text, "base_url", &url).unwrap();
        let twice = crate::codex_config::update_codex_toml_field(&once, "base_url", &url).unwrap();
        prop_assert_eq!(&once, &twice);
        let doc = parse(&once);
        prop_assert_eq!(doc["model_providers"][provider.as_str()]["base_url"].as_str(), Some(url.as_str()));
        prop_assert_eq!(crate::codex_config::extract_codex_base_url(&once), Some(url.clone()));
    }

    /// Editing one field leaves comments and every other table untouched.
    #[test]
    fn codex_field_update_preserves_comments_and_other_tables(
        (text, _provider, model) in codex_config(),
        new_model in "[a-z][a-z0-9.\\-]{0,12}",
    ) {
        let updated = crate::codex_config::update_codex_toml_field(&text, "model", &new_model).unwrap();
        for line in text.lines().filter(|l| l.starts_with('#')) {
            prop_assert!(updated.contains(line), "lost comment {:?}", line);
        }
        let before = parse(&text);
        let after = parse(&updated);
        prop_assert_eq!(after["model"].as_str(), Some(new_model.as_str()));
        prop_assert_eq!(after["profiles"].to_string(), before["profiles"].to_string());
        prop_assert_eq!(after["model_providers"].to_string(), before["model_providers"].to_string());
        let _ = model;
    }

    /// Clearing a field removes it; clearing it again is a no-op.
    #[test]
    fn codex_field_clear_is_idempotent((text, _provider, _model) in codex_config()) {
        let cleared = crate::codex_config::update_codex_toml_field(&text, "model", "").unwrap();
        prop_assert!(parse(&cleared).get("model").is_none());
        let again = crate::codex_config::update_codex_toml_field(&cleared, "model", "  ").unwrap();
        prop_assert_eq!(cleared, again);
    }

    /// parse(render(parse(x))) == parse(x): writing never changes meaning.
    #[test]
    fn codex_render_round_trip_is_stable((text, _provider, _model) in codex_config()) {
        let doc = parse(&text);
        let rendered = doc.to_string();
        prop_assert_eq!(parse(&rendered).to_string(), rendered);
    }

    /// The unified-session bucket is only injected into official (provider-less)
    /// configs, and stripping what was injected restores the original meaning.
    #[test]
    fn codex_unified_bucket_inject_strip_round_trip(
        model in "[a-z][a-z0-9.\\-]{0,12}",
        comment in "[a-zA-Z0-9 ]{0,20}",
    ) {
        let text = format!("# {comment}\nmodel = \"{model}\"\n");
        let injected = crate::codex_config::inject_codex_unified_session_bucket(&text).unwrap();
        prop_assert!(injected.contains("model_provider"));
        let again = crate::codex_config::inject_codex_unified_session_bucket(&injected).unwrap();
        prop_assert_eq!(&again, &injected, "injecting twice is a no-op");
        let stripped = crate::codex_config::strip_codex_unified_session_bucket(&injected).unwrap();
        let stripped_doc = parse(&stripped);
        prop_assert!(stripped_doc.get("model_provider").is_none());
        prop_assert_eq!(stripped_doc["model"].as_str(), Some(model.as_str()));
        let marker = format!("# {}", comment);
        prop_assert!(stripped.contains(&marker));
    }

    /// A config with a user-chosen provider is never re-routed.
    #[test]
    fn codex_unified_bucket_leaves_custom_providers_alone((text, _p, _m) in codex_config()) {
        let out = crate::codex_config::inject_codex_unified_session_bucket(&text).unwrap();
        prop_assert_eq!(out, text);
    }

    /// Grok: updating the API key of the selected model reads back and keeps the
    /// rest of the document.
    #[test]
    fn grok_update_api_key_reads_back(
        key in "sk-[A-Za-z0-9]{4,24}",
        comment in "[a-zA-Z0-9 ]{0,20}",
    ) {
        let text = format!(
            "# {comment}\n[models]\ndefault = \"m\"\n\n[model.\"m\"]\nmodel = \"grok-4\"\nname = \"xAI\"\nbase_url = \"https://api.x.ai/v1\"\napi_key = \"old\"\napi_backend = \"openai\"\ncontext_window = 128000\n\n[ui]\ntheme = \"dark\"\n"
        );
        let updated = crate::grok_config::update_api_key(&text, &key).unwrap();
        prop_assert_eq!(crate::grok_config::extract_inline_api_key(&updated), Some(key.clone()));
        let base_url = crate::grok_config::extract_base_url(&updated);
        prop_assert_eq!(base_url.as_deref(), Some("https://api.x.ai/v1"));
        let marker = format!("# {}", comment);
        prop_assert!(updated.contains(&marker));
        let updated_doc = parse(&updated);
        prop_assert_eq!(updated_doc["ui"]["theme"].as_str(), Some("dark"));
        let twice = crate::grok_config::update_api_key(&updated, &key).unwrap();
        prop_assert_eq!(twice, updated);
    }
}

proptest! {
    #![proptest_config(config(16))]

    /// opencode.json: arbitrary bytes on disk never panic the reader, and a
    /// write never replaces a file it could not parse.
    #[test]
    fn opencode_reader_tolerates_arbitrary_files(bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        let _home = crate::test_support::isolated_home();
        let path = crate::opencode_config::get_opencode_config_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        if crate::opencode_config::read_opencode_config().is_err() {
            let _ = crate::opencode_config::set_provider("p", serde_json::json!({"npm": "x"}));
            prop_assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    /// Provider writes round-trip for arbitrary ids and string values.
    #[test]
    fn opencode_provider_round_trip(
        id in "[a-zA-Z0-9_\\-]{1,16}",
        key in "[ -~]{0,30}",
    ) {
        let _home = crate::test_support::isolated_home();
        let value = serde_json::json!({"npm": "@ai-sdk/openai-compatible", "options": {"apiKey": key}});
        crate::opencode_config::set_provider(&id, value.clone()).unwrap();
        crate::opencode_config::set_provider(&id, value.clone()).unwrap();
        let providers = crate::opencode_config::get_providers().unwrap();
        prop_assert_eq!(providers.len(), 1);
        prop_assert_eq!(&providers[&id], &value);
    }
}
