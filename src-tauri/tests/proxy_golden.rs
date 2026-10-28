//! Golden tests for the Anthropic-facing protocol bridges.
//!
//! Each directory under `tests/fixtures/proxy/<bridge>/<case>/` is one case
//! (see `proxy::providers::testkit` for the layout). For every case this test
//! snapshots the converted upstream request, client response and recorded
//! usage with insta, re-splits stream inputs at every chunk boundary, and
//! checks the structural invariants of every output.
//!
//! Update snapshots after an intended behaviour change with
//! `INSTA_UPDATE=always cargo test --test proxy_golden` (or `cargo insta review`)
//! and review the diff.

use cc_switch_lib::proxy_testkit::{self as testkit, Bridge, Fixture};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// Minimum number of golden fixtures across all bridges.
const MIN_FIXTURES: usize = 30;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy")
}

fn load(bridge: Bridge) -> Vec<Fixture> {
    testkit::load_fixtures(&fixtures_root(), bridge)
        .unwrap_or_else(|error| panic!("failed to load {} fixtures: {error}", bridge.dir_name()))
}

fn assert_snapshots(fixture: &Fixture, outcome: &testkit::FixtureOutcome) {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(&fixture.dir);
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings.set_description(fixture.meta.intent.trim());
    settings.bind(|| {
        if let Some(text) = &outcome.upstream_request {
            insta::assert_snapshot!("upstream_request", text);
        }
        if let Some(text) = &outcome.client_response {
            insta::assert_snapshot!("client_response", text);
        }
        if let Some(text) = &outcome.usage {
            insta::assert_snapshot!("usage", text);
        }
    });
}

fn run_bridge(bridge: Bridge) {
    let fixtures = load(bridge);
    assert!(
        !fixtures.is_empty(),
        "no fixtures for {}",
        bridge.dir_name()
    );

    let mut report = Vec::new();
    for fixture in &fixtures {
        let outcome = testkit::run_fixture(fixture);
        for failure in &outcome.failures {
            report.push(format!("{}: {failure}", fixture.id()));
        }
        // Keep going after a snapshot mismatch so one run lists every case.
        if catch_unwind(AssertUnwindSafe(|| assert_snapshots(fixture, &outcome))).is_err() {
            report.push(format!(
                "{}: snapshot mismatch (see diff above)",
                fixture.id()
            ));
        }
    }

    assert!(
        report.is_empty(),
        "{} golden failure(s) for {}:\n{}",
        report.len(),
        bridge.dir_name(),
        report.join("\n")
    );
}

#[test]
fn golden_anthropic_openai_chat() {
    run_bridge(Bridge::AnthropicOpenAiChat);
}

#[test]
fn golden_anthropic_gemini() {
    run_bridge(Bridge::AnthropicGemini);
}

#[test]
fn golden_fixture_inventory() {
    let mut total = 0;
    for bridge in Bridge::ALL {
        let fixtures = load(bridge);
        total += fixtures.len();
        assert!(
            fixtures.iter().any(Fixture::has_stream),
            "{} needs at least one stream fixture",
            bridge.dir_name()
        );
        assert!(
            fixtures.iter().any(Fixture::has_non_stream),
            "{} needs at least one non-stream fixture",
            bridge.dir_name()
        );
        for fixture in &fixtures {
            assert!(
                !fixture.meta.intent.trim().is_empty() && !fixture.meta.source_commit.is_empty(),
                "{}: meta.toml needs intent and source_commit",
                fixture.id()
            );
            // Guards against git line-ending normalization of the raw bodies
            // (see tests/fixtures/proxy/.gitattributes).
            if fixture.name.contains("crlf") {
                let sse = fixture.upstream_sse.as_deref().unwrap_or_default();
                assert!(
                    sse.windows(2).any(|pair| pair == b"\r\n"),
                    "{}: CRLF fixture lost its CRLF line endings",
                    fixture.id()
                );
            }
        }
    }
    assert!(
        total >= MIN_FIXTURES,
        "expected at least {MIN_FIXTURES} golden fixtures, found {total}"
    );

    // Every bridge directory on disk must be one the testkit knows how to run.
    for entry in std::fs::read_dir(fixtures_root()).expect("read fixtures root") {
        let entry = entry.expect("dir entry");
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            Bridge::from_dir_name(&name).is_some(),
            "unknown bridge fixture directory `{name}`"
        );
    }
}
