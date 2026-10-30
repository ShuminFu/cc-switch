//! Golden tests for the protocol bridges (Anthropic and Responses clients).
//!
//! Each directory under `tests/fixtures/proxy/<bridge>/<case>/` is one case
//! (see `proxy::providers::testkit` for the layout). For every case this test
//! snapshots the converted upstream request, client response and recorded
//! usage with insta, re-splits stream inputs at every chunk boundary, and
//! checks the structural invariants of every output.
//!
//! Bridges migrated to codecs (see `proxy::codec::bridge`) run every fixture
//! under both conversion engines against the same snapshots, and the parity
//! checker compares the two engines' raw outputs case by case.
//!
//! Update snapshots after an intended behaviour change with
//! `INSTA_UPDATE=always cargo test --test proxy_golden` (or `cargo insta review`)
//! and review the diff.

use cc_switch_lib::proxy_testkit::{self as testkit, Bridge, CodecEngine, Fixture};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// Minimum number of golden fixtures across all bridges.
const MIN_FIXTURES: usize = 60;

/// Fix commits of the Responses bridges that must each be pinned by at least
/// one fixture citing them as `source_commit`.
const REGRESSION_COMMITS: [&str; 5] = ["6d316c0", "27ce0a5", "650905a", "613fef7", "f991726"];

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
    run_bridge_with(bridge, CodecEngine::Legacy);
}

fn run_bridge_with(bridge: Bridge, engine: CodecEngine) {
    let fixtures = load(bridge);
    assert!(
        !fixtures.is_empty(),
        "no fixtures for {}",
        bridge.dir_name()
    );

    let mut report = Vec::new();
    for fixture in &fixtures {
        let outcome = testkit::run_fixture_with(fixture, engine);
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
        "{} golden failure(s) for {} ({engine:?} engine):\n{}",
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
fn golden_anthropic_openai_chat_ir() {
    run_bridge_with(Bridge::AnthropicOpenAiChat, CodecEngine::Ir);
}

/// Prefix of the random tool-call ids the Gemini bridge synthesizes.
const SYNTHESIZED_ID_PREFIX: &[u8] = b"gemini_synth_";
/// Length of the UUID (simple encoding) after the prefix.
const SYNTHESIZED_ID_HEX_LEN: usize = 32;

/// Replace each distinct synthesized tool-call id with `gemini_synth_<n>`
/// (numbered by first appearance), so two runs compare byte for byte.
fn normalize_synthesized_ids(bytes: &[u8], seen: &mut Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = rest
        .windows(SYNTHESIZED_ID_PREFIX.len())
        .position(|window| window == SYNTHESIZED_ID_PREFIX)
    {
        let id_end = at + SYNTHESIZED_ID_PREFIX.len() + SYNTHESIZED_ID_HEX_LEN;
        out.extend_from_slice(&rest[..at]);
        if rest.len() < id_end {
            out.extend_from_slice(&rest[at..]);
            return out;
        }
        let id = rest[at..id_end].to_vec();
        let number = match seen.iter().position(|known| *known == id) {
            Some(number) => number,
            None => {
                seen.push(id);
                seen.len() - 1
            }
        };
        out.extend_from_slice(format!("gemini_synth_{number}").as_bytes());
        rest = &rest[id_end..];
    }
    out.extend_from_slice(rest);
    out
}

/// Raw client SSE output (bytes, or the error text of an `Err` item), with
/// synthesized tool-call ids normalized.
fn raw_stream(fixture: &Fixture, engine: CodecEngine) -> Option<Vec<Result<Vec<u8>, String>>> {
    let input = fixture.upstream_sse.as_deref()?;
    let mut seen = Vec::new();
    Some(
        testkit::stream_output_with(fixture, &[input], engine)
            .into_iter()
            .map(|item| {
                item.map(|bytes| normalize_synthesized_ids(&bytes, &mut seen))
                    .map_err(|e| e.to_string())
            })
            .collect(),
    )
}

/// Parity checker for a bridge migrated to codecs: every fixture must give the
/// same upstream request, the same raw client stream bytes, the same client
/// response, usage and check failures under the legacy converters and the IR
/// codecs. Split-point fuzzing runs inside `run_fixture_with` for both engines.
fn assert_codec_parity(bridge: Bridge) {
    let mut mismatches = Vec::new();
    for fixture in &load(bridge) {
        let legacy = testkit::run_fixture_with(fixture, CodecEngine::Legacy);
        let ir = testkit::run_fixture_with(fixture, CodecEngine::Ir);
        let fields = [
            (
                "upstream_request",
                &legacy.upstream_request,
                &ir.upstream_request,
            ),
            (
                "client_response",
                &legacy.client_response,
                &ir.client_response,
            ),
            ("usage", &legacy.usage, &ir.usage),
        ];
        for (name, legacy, ir) in fields {
            if legacy != ir {
                mismatches.push(format!(
                    "{}: {name} differs\n--- legacy\n{}\n--- ir\n{}",
                    fixture.id(),
                    legacy.as_deref().unwrap_or("<none>"),
                    ir.as_deref().unwrap_or("<none>")
                ));
            }
        }
        if legacy.failures != ir.failures {
            mismatches.push(format!(
                "{}: check failures differ\n  legacy: {:#?}\n  ir: {:#?}",
                fixture.id(),
                legacy.failures,
                ir.failures
            ));
        }
        if raw_stream(fixture, CodecEngine::Legacy) != raw_stream(fixture, CodecEngine::Ir) {
            mismatches.push(format!("{}: raw client stream bytes differ", fixture.id()));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} legacy/IR parity mismatch(es) for {}:\n{}",
        mismatches.len(),
        bridge.dir_name(),
        mismatches.join("\n")
    );
}

#[test]
fn codec_parity_anthropic_openai_chat() {
    assert_codec_parity(Bridge::AnthropicOpenAiChat);
}

#[test]
fn golden_anthropic_gemini() {
    run_bridge(Bridge::AnthropicGemini);
}

#[test]
fn golden_anthropic_gemini_ir() {
    run_bridge_with(Bridge::AnthropicGemini, CodecEngine::Ir);
}

#[test]
fn codec_parity_anthropic_gemini() {
    assert_codec_parity(Bridge::AnthropicGemini);
}

#[test]
fn golden_anthropic_responses() {
    run_bridge(Bridge::AnthropicResponses);
}

#[test]
fn golden_anthropic_responses_ir() {
    run_bridge_with(Bridge::AnthropicResponses, CodecEngine::Ir);
}

#[test]
fn codec_parity_anthropic_responses() {
    assert_codec_parity(Bridge::AnthropicResponses);
}

#[test]
fn golden_responses_chat() {
    run_bridge(Bridge::ResponsesChat);
}

#[test]
fn golden_responses_chat_ir() {
    run_bridge_with(Bridge::ResponsesChat, CodecEngine::Ir);
}

#[test]
fn codec_parity_responses_chat() {
    assert_codec_parity(Bridge::ResponsesChat);
}

#[test]
fn golden_responses_anthropic() {
    run_bridge(Bridge::ResponsesAnthropic);
}

#[test]
fn golden_responses_anthropic_ir() {
    run_bridge_with(Bridge::ResponsesAnthropic, CodecEngine::Ir);
}

#[test]
fn codec_parity_responses_anthropic() {
    assert_codec_parity(Bridge::ResponsesAnthropic);
}

#[test]
fn golden_fixture_inventory() {
    let mut total = 0;
    let mut commits = Vec::new();
    for bridge in Bridge::ALL {
        let fixtures = load(bridge);
        total += fixtures.len();
        commits.extend(
            fixtures
                .iter()
                .map(|fixture| fixture.meta.source_commit.clone()),
        );
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
        // Stream vs non-stream parity (incl. stop_reason) is only checked on
        // fixtures that carry both upstream bodies.
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.has_stream() && fixture.has_non_stream()),
            "{} needs at least one stream/non-stream parity fixture",
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
    for commit in REGRESSION_COMMITS {
        assert!(
            commits.iter().any(|cited| cited.starts_with(commit)),
            "no fixture cites regression commit {commit} as source_commit"
        );
    }

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
