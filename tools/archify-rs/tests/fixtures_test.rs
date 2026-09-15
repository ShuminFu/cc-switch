//! End-to-end coverage: every bundled specification parses, lays out, passes
//! the showcase battery, and renders a single-SVG standalone page; broken
//! specifications fail for the right reason.

use std::fs;
use std::path::PathBuf;

use archify_rs::layout;
use archify_rs::receipt::Digested;
use archify_rs::render;
use archify_rs::spec::{DiagramType, Spec};
use archify_rs::validate::{validate, Quality};

fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn compile_ok(kind: DiagramType, name: &str) -> (Spec, archify_rs::scene::Scene) {
    let spec = Spec::from_json(kind, &fixture(name)).expect("spec parses");
    let scene = layout::build(&spec).unwrap_or_else(|d| panic!("layout of {name}: {d:?}"));
    let v = validate(&scene, Quality::Showcase);
    assert!(
        v.ok,
        "{name} should pass showcase validation: {:#?}",
        v.diagnostics
    );
    assert!(v.checks.iter().all(|c| c.ok), "{name}: every check passes");
    assert_eq!(v.checks.len(), 11, "{name}: the full showcase battery runs");
    assert_eq!(v.errors(), 0);
    assert_eq!(v.warnings(), 0);
    (spec, scene)
}

fn assert_standalone_html(html: &str, title: &str) {
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert_eq!(
        html.matches("<svg xmlns=").count(),
        1,
        "exactly one diagram svg"
    );
    assert_eq!(html.matches("id=\"diagram\"").count(), 1);
    assert!(html.contains(&format!("<title>{title}</title>")));
    assert!(
        html.contains("prefers-color-scheme: dark"),
        "dark theme tokens are present"
    );
    assert!(
        html.contains("data-theme=\"dark\""),
        "explicit dark override is present"
    );
    assert!(html.contains("export-png"), "viewer exports are wired");
    assert!(
        !html.contains("NaN"),
        "no non-finite coordinates leaked into the markup"
    );
}

#[test]
fn architecture_fixture_passes_and_renders() {
    let (spec, scene) = compile_ok(
        DiagramType::Architecture,
        "upstream-review.architecture.json",
    );
    assert_eq!(scene.nodes.len(), 10);
    assert_eq!(scene.edges.len(), 10);
    assert_eq!(scene.boxes.len(), 2, "one region and one security group");
    let html = render::render_html(&spec, &scene);
    assert_standalone_html(&html, "CC Switch Upstream Review — v3.17.0 to v3.20.3");
    assert!(html.contains("class=\"box region"));
    assert!(html.contains("class=\"box security-group"));
    assert_eq!(html.matches("<article class=\"card\">").count(), 4);
    assert_eq!(html.matches("class=\"view\"").count(), 3);
}

#[test]
fn sequence_fixture_passes_and_renders() {
    let (spec, scene) = compile_ok(DiagramType::Sequence, "proxy-bridge.sequence.json");
    assert_eq!(scene.nodes.len(), 6);
    assert_eq!(scene.lines.len(), 6, "one lifeline per participant");
    assert_eq!(scene.bars.len(), 6);
    assert_eq!(scene.edges.len(), 8);
    assert_eq!(scene.timeline, Some((160.0, 480.0 - 83.0)));
    let html = render::render_html(&spec, &scene);
    assert_standalone_html(
        &html,
        "CC Switch Local Proxy — Messages to Responses Bridge",
    );
    assert_eq!(html.matches("class=\"bar kind-").count(), 6);
}

#[test]
fn workflow_fixture_passes_and_renders() {
    let (spec, scene) = compile_ok(DiagramType::Workflow, "catchup.workflow.json");
    assert_eq!(scene.nodes.len(), 8);
    assert_eq!(scene.edges.len(), 7);
    let lanes = scene
        .boxes
        .iter()
        .filter(|b| {
            matches!(
                b.kind,
                archify_rs::scene::BoxKind::Lane | archify_rs::scene::BoxKind::ExceptionLane
            )
        })
        .count();
    assert_eq!(lanes, 2);
    let phases = scene
        .boxes
        .iter()
        .filter(|b| b.kind == archify_rs::scene::BoxKind::Phase)
        .count();
    assert_eq!(phases, 4);
    // Main-path edges between adjacent same-lane nodes are straight lines.
    let e = scene.edges.iter().find(|e| e.id == "e-fetch-ff").unwrap();
    assert_eq!(e.points.len(), 2, "{:?}", e.points);
    let html = render::render_html(&spec, &scene);
    assert_standalone_html(&html, "Catching This Fork Up to Upstream v3.20.3");
    assert!(html.contains("EX / Upgrade risks"));
}

#[test]
fn route_through_a_node_is_rejected() {
    let spec = Spec::from_json(
        DiagramType::Architecture,
        &fixture("broken.architecture.json"),
    )
    .unwrap();
    let scene = layout::build(&spec).unwrap();
    let v = validate(&scene, Quality::Showcase);
    assert!(!v.ok);
    let codes: Vec<&str> = v.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"layout/route-crosses-node"), "{codes:?}");
    assert!(v
        .checks
        .iter()
        .any(|c| c.name == "relationship_crossings" && !c.ok));
}

#[test]
fn dangling_reference_fails_layout() {
    let spec = Spec::from_json(DiagramType::Workflow, &fixture("dangling.workflow.json")).unwrap();
    let err = layout::build(&spec).expect_err("layout must fail");
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code, "references/unknown-id");
    assert!(err[0].message.contains("ghost"));
}

#[test]
fn spec_rejects_wrong_type_unknown_fields_and_duplicates() {
    let bytes = fixture("catchup.workflow.json");
    let err = Spec::from_json(DiagramType::Sequence, &bytes).unwrap_err();
    assert!(err.contains("diagram_type"), "{err}");

    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["nodes"][0]["colour"] = serde_json::json!("red");
    let err = Spec::from_json(DiagramType::Workflow, v.to_string().as_bytes()).unwrap_err();
    assert!(err.contains("unknown field"), "{err}");

    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["nodes"][1]["id"] = v["nodes"][0]["id"].clone();
    let err = Spec::from_json(DiagramType::Workflow, v.to_string().as_bytes()).unwrap_err();
    assert!(err.contains("duplicate node id"), "{err}");

    let mut v: serde_json::Value =
        serde_json::from_slice(&fixture("proxy-bridge.sequence.json")).unwrap();
    v["meta"]["viewBox"] = serde_json::json!([1000, 300]);
    let err = Spec::from_json(DiagramType::Sequence, v.to_string().as_bytes()).unwrap_err();
    assert!(err.contains("480x480"), "{err}");
}

#[test]
fn rendering_is_deterministic() {
    let (spec, scene) = compile_ok(
        DiagramType::Architecture,
        "upstream-review.architecture.json",
    );
    let a = render::render_html(&spec, &scene);
    let b = render::render_html(&spec, &scene);
    assert_eq!(
        Digested::of(a.as_bytes()).sha256,
        Digested::of(b.as_bytes()).sha256
    );
}

#[test]
fn standard_profile_skips_readability_only() {
    let spec = Spec::from_json(
        DiagramType::Architecture,
        &fixture("upstream-review.architecture.json"),
    )
    .unwrap();
    let scene = layout::build(&spec).unwrap();
    let v = validate(&scene, Quality::Standard);
    assert!(v.ok);
    assert_eq!(v.checks.len(), 10);
    assert!(v.checks.iter().all(|c| c.name != "desktop_readability"));
}
