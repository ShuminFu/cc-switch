//! Guards the contract between the backend and the Rust frontend
//! (`crates/cc-switch-contract`).

use std::collections::BTreeSet;

/// Every command the frontend invokes must be registered in
/// `tauri::generate_handler!`, and the contract's serde types must accept
/// what the backend serializes.
fn registered_commands() -> BTreeSet<String> {
    let src = include_str!("../src/lib.rs");
    let start = src
        .find("generate_handler![")
        .expect("generate_handler! block in lib.rs");
    let block = &src[start..];
    let end = block.find("])").expect("end of generate_handler! block");
    block[..end]
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_end_matches(',');
            if line.is_empty() || line.starts_with("//") || line.contains("generate_handler") {
                return None;
            }
            Some(line.rsplit("::").next().unwrap_or(line).to_string())
        })
        .collect()
}

#[test]
fn every_contract_command_is_registered() {
    let registered = registered_commands();
    assert!(
        registered.len() > 200,
        "parsed {} commands",
        registered.len()
    );
    let missing: Vec<&str> = cc_switch_contract::commands::ALL
        .iter()
        .copied()
        .filter(|c| !registered.contains(*c))
        .collect();
    assert!(
        missing.is_empty(),
        "commands used by the frontend but not registered: {missing:?}"
    );
}

#[test]
fn settings_round_trip_through_contract_type() {
    let backend = cc_switch_lib::AppSettings::default();
    let json = serde_json::to_value(&backend).unwrap();
    let contract: cc_switch_contract::AppSettings = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(contract.show_in_tray, backend.show_in_tray);
    assert_eq!(serde_json::to_value(&contract).unwrap(), json);
    let back: cc_switch_lib::AppSettings =
        serde_json::from_value(serde_json::to_value(&contract).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), json);
}

#[test]
fn provider_round_trip_through_contract_type() {
    let backend = cc_switch_lib::Provider::with_id(
        "p1".to_string(),
        "Test".to_string(),
        serde_json::json!({"env": {"ANTHROPIC_BASE_URL": "https://example.com"}}),
        Some("https://example.com".to_string()),
    );
    let json = serde_json::to_value(&backend).unwrap();
    let contract: cc_switch_contract::Provider = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(contract.id, "p1");
    assert_eq!(serde_json::to_value(&contract).unwrap(), json);
}
