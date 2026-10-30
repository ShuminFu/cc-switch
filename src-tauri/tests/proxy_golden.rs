//! Integrity checks for the proxy golden-fixture corpus (PROV-20 / TEST-02 / PRODUCT-04).
//!
//! The conversion functions live in the crate-private `proxy` module, so the golden
//! comparisons themselves run in-crate: `cargo test --lib proxy::testing` (see
//! `src/proxy/testing.rs`; `UPDATE_GOLDEN=1` rewrites the expected files). This
//! integration test keeps the corpus itself honest so a broken or half-added fixture is
//! caught even when only the integration tests run: every case has a parseable input and
//! expected file, SSE inputs are well-formed, and each bridge keeps its minimum coverage.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

const BRIDGES: &[&str] = &[
    "anthropic_chat",
    "anthropic_responses",
    "anthropic_gemini",
    "codex_chat",
    "codex_anthropic",
];
const KINDS: &[&str] = &["request", "response", "stream"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy")
}

/// (bridge, kind) -> case names found on disk.
fn cases() -> BTreeMap<(String, String), Vec<String>> {
    let mut out = BTreeMap::new();
    for bridge in BRIDGES {
        for kind in KINDS {
            let dir = root().join(bridge).join(kind);
            let mut names = Vec::new();
            for entry in std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("missing fixture dir {}: {e}", dir.display()))
            {
                let name = entry.unwrap().file_name().to_string_lossy().to_string();
                if let Some(case) = name
                    .strip_suffix(".input.json")
                    .or_else(|| name.strip_suffix(".input.sse"))
                {
                    names.push(case.to_string());
                }
            }
            names.sort();
            out.insert((bridge.to_string(), kind.to_string()), names);
        }
    }
    out
}

#[test]
fn every_case_has_parseable_input_and_expected() {
    for ((bridge, kind), names) in cases() {
        for case in names {
            let dir = root().join(&bridge).join(&kind);
            let expected = dir.join(format!("{case}.expected.json"));
            let text = std::fs::read_to_string(&expected)
                .unwrap_or_else(|e| panic!("{}: {e}", expected.display()));
            let value: Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{}: invalid JSON: {e}", expected.display()));
            if kind == "stream" {
                assert!(
                    value.as_array().is_some_and(|events| !events.is_empty()),
                    "{}: a stream expectation is a non-empty event array",
                    expected.display()
                );
                let input = dir.join(format!("{case}.input.sse"));
                let sse = std::fs::read_to_string(&input).unwrap();
                for block in sse.split("\n\n").filter(|b| !b.trim().is_empty()) {
                    for line in block.lines() {
                        assert!(
                            line.starts_with("data:")
                                || line.starts_with("event:")
                                || line.starts_with(':'),
                            "{}: unexpected SSE line {line:?}",
                            input.display()
                        );
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data != "[DONE]" {
                                serde_json::from_str::<Value>(data).unwrap_or_else(|e| {
                                    panic!("{}: bad data line {data:?}: {e}", input.display())
                                });
                            }
                        }
                    }
                }
            } else {
                let input = dir.join(format!("{case}.input.json"));
                serde_json::from_str::<Value>(&std::fs::read_to_string(&input).unwrap())
                    .unwrap_or_else(|e| panic!("{}: invalid JSON: {e}", input.display()));
            }
        }
    }
}

#[test]
fn corpus_keeps_minimum_coverage() {
    let cases = cases();
    let total: usize = cases.values().map(Vec::len).sum();
    assert!(total >= 40, "golden corpus shrank to {total} cases");
    for ((bridge, kind), names) in &cases {
        assert!(
            names.len() >= 3,
            "{bridge}/{kind} has only {} cases",
            names.len()
        );
    }
    // Gateway quirks referenced by converter comments and findings.
    for required in [
        "anthropic_chat/stream/tool_calls_without_index",
        "anthropic_chat/stream/double_finish_reason",
        "anthropic_chat/stream/transport_error_mid_stream",
        "anthropic_gemini/stream/parallel_no_id_calls",
        "codex_chat/stream/tool_calls_without_index",
        "codex_anthropic/stream/error_event",
    ] {
        let mut parts = required.split('/');
        let key = (
            parts.next().unwrap().to_string(),
            parts.next().unwrap().to_string(),
        );
        let case = parts.next().unwrap();
        assert!(
            cases
                .get(&key)
                .is_some_and(|names| names.iter().any(|n| n == case)),
            "missing required fixture {required}"
        );
    }
}
