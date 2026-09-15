//! Geometry checks over a resolved [`Scene`]. Every check is reported, pass
//! or fail, so a receipt always lists the full battery.

use serde::Serialize;

use crate::diag::{Check, Diagnostic, Severity};
use crate::geom::{self, Rect};
use crate::scene::{Scene, NODE_SUBLABEL_PX};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Standard,
    Showcase,
}

impl Quality {
    pub fn parse(s: &str) -> Option<Quality> {
        match s {
            "standard" => Some(Quality::Standard),
            "showcase" => Some(Quality::Showcase),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Quality::Standard => "standard",
            Quality::Showcase => "showcase",
        }
    }
}

/// Minimum clear gap between a relationship label and any other route.
pub const LABEL_CLEARANCE_PX: f64 = 4.0;
/// Desktop readability: text must project to at least this many CSS pixels
/// when the diagram is scaled into a 930px reading column.
pub const MIN_PROJECTED_FONT_PX: f64 = 6.0;
pub const READING_WIDTH_PX: f64 = 930.0;

#[derive(Debug, Clone, Serialize)]
pub struct Validation {
    pub ok: bool,
    pub profile: Quality,
    pub checks: Vec<Check>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Validation {
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn checks_passed(&self) -> usize {
        self.checks.iter().filter(|c| c.ok).count()
    }

    /// A receipt for a spec that never produced a scene.
    pub fn failed_layout(profile: Quality, diagnostics: Vec<Diagnostic>) -> Validation {
        let mut references = Check::new("references");
        for d in &diagnostics {
            references.fail(d.message.clone());
        }
        Validation {
            ok: false,
            profile,
            checks: vec![references],
            diagnostics,
        }
    }
}

fn rect_json(r: &Rect) -> serde_json::Value {
    serde_json::json!([r.x, r.y, r.w, r.h])
}

pub fn validate(scene: &Scene, profile: Quality) -> Validation {
    let mut checks = Vec::new();
    let mut diags: Vec<Diagnostic> = Vec::new();

    checks.push(Check::new("references"));

    // finite_svg -------------------------------------------------------------
    let mut finite = Check::new("finite_svg");
    let all_finite = scene.nodes.iter().all(|n| n.rect.is_finite())
        && scene.boxes.iter().all(|b| b.rect.is_finite())
        && scene.edges.iter().all(|e| {
            e.points
                .iter()
                .all(|p| p[0].is_finite() && p[1].is_finite())
        })
        && scene.view_w.is_finite()
        && scene.view_h.is_finite();
    if !all_finite {
        finite.fail("a coordinate is NaN or infinite");
        diags.push(Diagnostic::error(
            "geometry/non-finite",
            "the scene contains a non-finite coordinate",
        ));
    }
    checks.push(finite);

    // orthogonal_arrows -------------------------------------------------------
    let mut ortho = Check::new("orthogonal_arrows");
    for e in &scene.edges {
        for (i, s) in geom::segments(&e.points).iter().enumerate() {
            if !s.is_orthogonal() {
                let msg = format!(
                    "relationship \"{}\" segment {i} is diagonal ({:?} -> {:?})",
                    e.id, s.a, s.b
                );
                ortho.fail(msg.clone());
                diags.push(Diagnostic::error("geometry/diagonal-segment", msg));
            }
            if s.length() > 0.0 && s.length() < 8.0 && e.points.len() > 2 {
                let msg = format!(
                    "relationship \"{}\" segment {i} is a {:.1}px micro-segment",
                    e.id,
                    s.length()
                );
                ortho.fail(msg.clone());
                diags.push(Diagnostic::error("geometry/micro-segment", msg));
            }
        }
    }
    checks.push(ortho);

    // node_overlap ------------------------------------------------------------
    let mut overlap = Check::new("node_overlap");
    for (i, a) in scene.nodes.iter().enumerate() {
        for b in scene.nodes.iter().skip(i + 1) {
            if a.rect.intersects(&b.rect) {
                let msg = format!("nodes \"{}\" and \"{}\" overlap", a.id, b.id);
                overlap.fail(msg.clone());
                diags.push(Diagnostic::error("layout/node-overlap", msg).with_evidence(
                    serde_json::json!({"a": rect_json(&a.rect), "b": rect_json(&b.rect)}),
                ));
            }
        }
    }
    checks.push(overlap);

    // label_fit ---------------------------------------------------------------
    let mut fit = Check::new("label_fit");
    for n in &scene.nodes {
        // 12px accent bar and padding on the left, 12px padding on the right.
        let inner = n.rect.w - 24.0;
        let lw = geom::text_width(&n.label, n.label_px);
        if lw > inner {
            let msg = format!(
                "Label \"{}\" (~{lw:.0}px) is wider than node \"{}\" ({:.0}px) — shorten the label or increase the node width.",
                n.label, n.id, n.rect.w
            );
            fit.fail(msg.clone());
            diags.push(Diagnostic::error("layout/label-fit", msg));
        }
        if let Some(sub) = &n.sublabel {
            let sw = geom::text_width(sub, n.sublabel_px);
            if sw > inner {
                let msg = format!(
                    "Sublabel \"{sub}\" (~{sw:.0}px) is wider than node \"{}\" ({:.0}px) — shorten it or increase the node width.",
                    n.id, n.rect.w
                );
                fit.fail(msg.clone());
                diags.push(Diagnostic::error("layout/label-fit", msg));
            }
        }
    }
    checks.push(fit);

    // relationship_crossings --------------------------------------------------
    let mut crossings = Check::new("relationship_crossings");
    for e in &scene.edges {
        for (i, s) in geom::segments(&e.points).iter().enumerate() {
            for n in &scene.nodes {
                if n.id == e.from || n.id == e.to {
                    continue;
                }
                if s.crosses_rect(&n.rect) {
                    let msg = format!(
                        "relationship \"{}\" ({} -> {}) segment {i} crosses unrelated node \"{}\" — reroute with via/fromSide/toSide or move the node.",
                        e.id, e.from, e.to, n.id
                    );
                    crossings.fail(msg.clone());
                    diags.push(
                        Diagnostic::error("layout/route-crosses-node", msg).with_evidence(
                            serde_json::json!({"segment": [s.a, s.b], "node": rect_json(&n.rect)}),
                        ),
                    );
                }
            }
        }
    }
    checks.push(crossings);

    // relationship_corridors (two routes sharing one collinear run) ----------
    let mut corridors = Check::new("relationship_corridors");
    for (i, a) in scene.edges.iter().enumerate() {
        for b in scene.edges.iter().skip(i + 1) {
            for sa in geom::segments(&a.points) {
                for sb in geom::segments(&b.points) {
                    let shared = if sa.is_horizontal() && sb.is_horizontal() {
                        (sa.a[1] - sb.a[1]).abs() < 1.0 && {
                            let (a0, a1) = (sa.a[0].min(sa.b[0]), sa.a[0].max(sa.b[0]));
                            let (b0, b1) = (sb.a[0].min(sb.b[0]), sb.a[0].max(sb.b[0]));
                            a1.min(b1) - a0.max(b0) > 8.0
                        }
                    } else if sa.is_vertical() && sb.is_vertical() {
                        (sa.a[0] - sb.a[0]).abs() < 1.0 && {
                            let (a0, a1) = (sa.a[1].min(sa.b[1]), sa.a[1].max(sa.b[1]));
                            let (b0, b1) = (sb.a[1].min(sb.b[1]), sb.a[1].max(sb.b[1]));
                            a1.min(b1) - a0.max(b0) > 8.0
                        }
                    } else {
                        false
                    };
                    if shared {
                        let msg = format!(
                            "relationships \"{}\" and \"{}\" share an ambiguous collinear corridor — separate them with via or a channel.",
                            a.id, b.id
                        );
                        corridors.fail(msg.clone());
                        diags.push(Diagnostic::error("layout/ambiguous-corridor", msg));
                    }
                }
            }
        }
    }
    checks.push(corridors);

    // label_route_clearance ---------------------------------------------------
    let mut clearance = Check::new("label_route_clearance");
    for (i, e) in scene.edges.iter().enumerate() {
        let Some(label) = &e.label else { continue };
        for n in &scene.nodes {
            if label.rect.intersects(&n.rect) {
                let msg = format!(
                    "Label \"{}\" overlaps node \"{}\" — adjust labelDx/labelDy/labelSegment or set labelAt.\n  label rect: {:?}\n  node rect: {:?}",
                    label.text, n.id, rect_json(&label.rect), rect_json(&n.rect)
                );
                clearance.fail(msg.clone());
                diags.push(Diagnostic::error("layout/label-overlaps-node", msg));
            }
        }
        for (j, other) in scene.edges.iter().enumerate() {
            if i == j {
                continue;
            }
            for (k, s) in geom::segments(&other.points).iter().enumerate() {
                let d = s.distance_to_rect(&label.rect);
                if d < LABEL_CLEARANCE_PX {
                    let msg = format!(
                        "Label \"{}\" on \"{}\" is {d:.1}px from relationship \"{}\" segment {k} (minimum {LABEL_CLEARANCE_PX}px) — move the label or reroute the other relationship.",
                        label.text, e.id, other.id
                    );
                    clearance.fail(msg.clone());
                    diags.push(
                        Diagnostic::error("composition/label-route-clearance", msg).with_evidence(
                            serde_json::json!({"label": rect_json(&label.rect), "segment": [s.a, s.b], "clearancePx": d}),
                        ),
                    );
                }
            }
            if let Some(ol) = &other.label {
                if j > i && label.rect.intersects(&ol.rect) {
                    let msg = format!(
                        "Labels \"{}\" and \"{}\" overlap each other.",
                        label.text, ol.text
                    );
                    clearance.fail(msg.clone());
                    diags.push(Diagnostic::error("composition/label-overlap", msg));
                }
            }
        }
    }
    checks.push(clearance);

    // containment -------------------------------------------------------------
    let mut contain = Check::new("viewbox_containment");
    let view = Rect::new(0.0, 0.0, scene.view_w, scene.view_h);
    let inside = |r: &Rect| {
        r.x >= view.x && r.y >= view.y && r.right() <= view.right() && r.bottom() <= view.bottom()
    };
    for n in &scene.nodes {
        if !inside(&n.rect) {
            let msg = format!(
                "node \"{}\" lies outside the {}x{} viewBox",
                n.id, scene.view_w, scene.view_h
            );
            contain.fail(msg.clone());
            diags.push(Diagnostic::error("layout/outside-viewbox", msg));
        }
    }
    for b in &scene.boxes {
        if !inside(&b.rect) {
            let msg = format!("container \"{}\" lies outside the viewBox", b.label);
            contain.fail(msg.clone());
            diags.push(Diagnostic::error("layout/outside-viewbox", msg));
        }
    }
    for e in &scene.edges {
        if let Some(l) = &e.label {
            if !inside(&l.rect) {
                let msg = format!("label \"{}\" lies outside the viewBox", l.text);
                contain.fail(msg.clone());
                diags.push(Diagnostic::error("layout/outside-viewbox", msg));
            }
        }
    }
    checks.push(contain);

    // timeline (sequence) -----------------------------------------------------
    let mut timeline = Check::new("timeline");
    if let Some((top, bottom)) = scene.timeline {
        for e in &scene.edges {
            let y = e.points[0][1];
            if y < top || y > bottom {
                let msg = format!(
                    "Message \"{}\" sits outside the readable timeline — keep y between {top:.0} and {bottom:.0}.",
                    e.label.as_ref().map(|l| l.text.as_str()).unwrap_or(&e.id)
                );
                timeline.fail(msg.clone());
                diags.push(Diagnostic::error("sequence/timeline", msg));
            }
        }
        for b in &scene.bars {
            if b.rect.y < top - 10.0 || b.rect.bottom() > bottom + 12.0 {
                let msg = format!(
                    "activation on \"{}\" extends beyond the readable timeline",
                    b.owner
                );
                timeline.fail(msg.clone());
                diags.push(Diagnostic::error("sequence/timeline", msg));
            }
        }
    }
    checks.push(timeline);

    // desktop_readability (showcase only) ------------------------------------
    if profile == Quality::Showcase {
        let mut readability = Check::new("desktop_readability");
        let scale = READING_WIDTH_PX / scene.view_w.max(1.0);
        let mut worst: Option<(&str, f64, f64)> = None;
        for n in &scene.nodes {
            let font = if n.sublabel.is_some() {
                n.sublabel_px
            } else {
                n.label_px
            };
            let projected = font * scale;
            if worst.is_none_or(|w| projected < w.2) {
                worst = Some((n.sublabel.as_deref().unwrap_or(&n.label), font, projected));
            }
        }
        if let Some((text, font, projected)) = worst {
            if projected < MIN_PROJECTED_FONT_PX {
                let msg = format!(
                    "Node text \"{text}\" projects to {projected:.2}px at a 1440px desktop viewport (minimum {MIN_PROJECTED_FONT_PX}px): reduce the viewBox width below {:.0} or shorten node copy.",
                    font * READING_WIDTH_PX / MIN_PROJECTED_FONT_PX
                );
                readability.fail(msg.clone());
                diags.push(
                    Diagnostic::error("composition/desktop-readability", msg).with_evidence(
                        serde_json::json!({
                            "viewBoxWidth": scene.view_w,
                            "scale": scale,
                            "sourceFontPx": font,
                            "projectedFontPx": projected,
                            "minimumProjectedFontPx": MIN_PROJECTED_FONT_PX,
                            "sublabelFontPx": NODE_SUBLABEL_PX,
                        }),
                    ),
                );
            }
        }
        checks.push(readability);
    }

    let ok = diags.iter().all(|d| d.severity != Severity::Error);
    Validation {
        ok,
        profile,
        checks,
        diagnostics: diags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::*;
    use crate::spec::{ComponentType, Variant};

    fn node(id: &str, x: f64, y: f64) -> SceneNode {
        SceneNode {
            id: id.into(),
            rect: Rect::new(x, y, 120.0, 60.0),
            kind: ComponentType::Backend,
            label: "Svc".into(),
            sublabel: None,
            tag: None,
            label_px: NODE_LABEL_PX,
            sublabel_px: NODE_SUBLABEL_PX,
        }
    }

    fn edge(
        id: &str,
        from: &str,
        to: &str,
        points: Vec<[f64; 2]>,
        label: Option<SceneLabel>,
    ) -> SceneEdge {
        SceneEdge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            points,
            label,
            variant: Variant::Default,
            role: EdgeRole::Branch,
            width: 1.5,
        }
    }

    #[test]
    fn detects_route_through_unrelated_node() {
        let mut scene = Scene {
            view_w: 600.0,
            view_h: 300.0,
            ..Default::default()
        };
        scene.nodes = vec![
            node("a", 0.0, 100.0),
            node("mid", 200.0, 100.0),
            node("b", 400.0, 100.0),
        ];
        scene.edges = vec![edge(
            "a-b",
            "a",
            "b",
            vec![[120.0, 130.0], [400.0, 130.0]],
            None,
        )];
        let v = validate(&scene, Quality::Showcase);
        assert!(!v.ok);
        assert!(v
            .diagnostics
            .iter()
            .any(|d| d.code == "layout/route-crosses-node"));
        assert!(v
            .checks
            .iter()
            .any(|c| c.name == "relationship_crossings" && !c.ok));
    }

    #[test]
    fn clean_scene_passes_all_checks() {
        let mut scene = Scene {
            view_w: 500.0,
            view_h: 300.0,
            ..Default::default()
        };
        scene.nodes = vec![node("a", 0.0, 100.0), node("b", 300.0, 100.0)];
        let label = crate::layout::place_label(
            &[[120.0, 130.0], [300.0, 130.0]],
            "calls",
            None,
            None,
            0.0,
            0.0,
        );
        scene.edges = vec![edge(
            "a-b",
            "a",
            "b",
            vec![[120.0, 130.0], [300.0, 130.0]],
            Some(label),
        )];
        let v = validate(&scene, Quality::Showcase);
        assert!(v.ok, "{:?}", v.diagnostics);
        assert_eq!(v.checks.len(), 11);
        assert!(v.checks.iter().all(|c| c.ok));
    }

    #[test]
    fn readability_fails_on_wide_viewbox() {
        let mut scene = Scene {
            view_w: 3000.0,
            view_h: 300.0,
            ..Default::default()
        };
        let mut n = node("a", 0.0, 100.0);
        n.sublabel = Some("context".into());
        scene.nodes = vec![n];
        let v = validate(&scene, Quality::Showcase);
        assert!(v
            .diagnostics
            .iter()
            .any(|d| d.code == "composition/desktop-readability"));
        let v2 = validate(&scene, Quality::Standard);
        assert!(v2.ok, "standard profile skips readability");
    }
}
