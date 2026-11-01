//! IPC error contract (TEST-26).
//!
//! Snapshots the exact JSON that `AppError` serialises to when a command
//! rejects, into `tests/contracts/app_error.json` at the repository root. The
//! frontend MSW helper `tauriError()` (tests/msw/tauriError.ts) builds its
//! rejections from the same file, so mocked error paths match what the real
//! backend sends.
//!
//! Regenerate after an intentional change with
//! `UPDATE_CONTRACTS=1 cargo test --test contracts`.

use std::path::PathBuf;

use cc_switch_lib::AppError;
use serde_json::{json, Map, Value};

fn contract_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("contracts")
        .join("app_error.json")
}

fn samples() -> Vec<(&'static str, AppError)> {
    vec![
        ("config", AppError::Config("bad value".into())),
        (
            "invalidInput",
            AppError::InvalidInput("name is empty".into()),
        ),
        ("message", AppError::Message("Provider not found".into())),
        (
            "httpStatus",
            AppError::HttpStatus {
                status: 429,
                body: "rate limited".into(),
            },
        ),
        (
            "localized",
            AppError::localized(
                "sync.conflict.remote_changed",
                "远端数据自上次同步后已被其他设备更新",
                "the remote data changed since the last sync",
            ),
        ),
        ("database", AppError::Database("database is locked".into())),
        ("allProvidersCircuitOpen", AppError::AllProvidersCircuitOpen),
        ("noProvidersConfigured", AppError::NoProvidersConfigured),
    ]
}

#[test]
fn app_error_wire_shape_matches_contract_fixture() {
    let mut actual = Map::new();
    for (name, err) in samples() {
        actual.insert(
            name.to_string(),
            serde_json::to_value(&err).expect("serialize AppError"),
        );
    }
    let actual = Value::Object(actual);

    // Every payload is `{ code, message, params? }` with no other keys.
    for (name, payload) in actual.as_object().unwrap() {
        let obj = payload.as_object().unwrap();
        assert!(obj["code"].is_string(), "{name}");
        assert!(obj["message"].is_string(), "{name}");
        for key in obj.keys() {
            assert!(
                ["code", "message", "params"].contains(&key.as_str()),
                "{name}: unexpected key {key}"
            );
        }
    }
    assert_eq!(
        actual["localized"]["code"],
        json!("sync.conflict.remote_changed")
    );
    assert!(actual["noProvidersConfigured"].get("params").is_none());

    let path = contract_path();
    let pretty = serde_json::to_string_pretty(&actual).unwrap() + "\n";
    if std::env::var_os("UPDATE_CONTRACTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &pretty).unwrap();
        return;
    }
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
    )
    .expect("contract fixture is JSON");
    assert_eq!(
        actual, expected,
        "AppError wire shape changed; rerun with UPDATE_CONTRACTS=1 and update the frontend if needed"
    );
}
