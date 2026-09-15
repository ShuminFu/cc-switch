//! Emits a self-contained HTML document: inline SVG with CSS-token styling,
//! light/dark theme, pan/zoom, guided views, legend, conclusion cards and
//! truthful SVG/PNG export (the exporter resolves CSS variables so the file
//! looks identical outside the page).

use std::fmt::Write as _;

use crate::geom::Rect;
use crate::scene::{BoxKind, EdgeRole, Scene, SceneEdge, SceneNode};
use crate::spec::{Spec, Variant};

pub const GENERATOR: &str = concat!("archify-rs ", env!("CARGO_PKG_VERSION"));

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

fn role_class(role: EdgeRole) -> &'static str {
    match role {
        EdgeRole::Main => "main",
        EdgeRole::Branch => "branch",
        EdgeRole::Async => "async",
        EdgeRole::Return => "return",
        EdgeRole::Error => "error",
    }
}

fn box_class(kind: BoxKind) -> &'static str {
    match kind {
        BoxKind::Region => "region",
        BoxKind::SecurityGroup => "security-group",
        BoxKind::Lane => "lane",
        BoxKind::ExceptionLane => "lane exception",
        BoxKind::Group => "group",
        BoxKind::Phase => "phase",
        BoxKind::TimeBand => "band",
    }
}

fn path_d(points: &[[f64; 2]]) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let _ = write!(
            d,
            "{}{} {}",
            if i == 0 { "M" } else { " L" },
            num(p[0]),
            num(p[1])
        );
    }
    d
}

fn write_node(out: &mut String, n: &SceneNode) {
    let r = n.rect;
    let _ = writeln!(
        out,
        "<g class=\"node kind-{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"10\"/>",
        n.kind.css(),
        escape(&n.id),
        num(r.x),
        num(r.y),
        num(r.w),
        num(r.h)
    );
    let _ = writeln!(
        out,
        "<rect class=\"accent\" x=\"{}\" y=\"{}\" width=\"4\" height=\"{}\" rx=\"2\"/>",
        num(r.x + 8.0),
        num(r.y + 12.0),
        num(r.h - 24.0)
    );
    let label_y = if n.sublabel.is_some() {
        r.y + r.h / 2.0 - 6.0
    } else {
        r.y + r.h / 2.0
    };
    let _ = writeln!(
        out,
        "<text class=\"label\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"middle\" font-size=\"{}\">{}</text>",
        num(r.cx()),
        num(label_y),
        num(n.label_px),
        escape(&n.label)
    );
    if let Some(sub) = &n.sublabel {
        let _ = writeln!(
            out,
            "<text class=\"sublabel\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"middle\" font-size=\"{}\">{}</text>",
            num(r.cx()),
            num(r.y + r.h / 2.0 + 11.0),
            num(n.sublabel_px),
            escape(sub)
        );
    }
    if let Some(tag) = &n.tag {
        let tw = crate::geom::text_width(tag, 8.0) + 14.0;
        let _ = writeln!(
            out,
            "<g class=\"tag\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"14\" rx=\"7\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"middle\" font-size=\"8\">{}</text></g>",
            num(r.right() - tw - 6.0),
            num(r.y - 7.0),
            num(tw),
            num(r.right() - 6.0 - tw / 2.0),
            num(r.y),
            escape(tag)
        );
    }
    out.push_str("</g>\n");
}

fn write_edge(out: &mut String, e: &SceneEdge) {
    let role = role_class(e.role);
    let dashed =
        matches!(e.role, EdgeRole::Async | EdgeRole::Return) || e.variant == Variant::Dashed;
    let _ = writeln!(
        out,
        "<path class=\"edge role-{role} variant-{}{}\" data-id=\"{}\" data-from=\"{}\" data-to=\"{}\" d=\"{}\" stroke-width=\"{}\" marker-end=\"url(#arrow-{role})\"/>",
        e.variant.css(),
        if dashed { " dashed" } else { "" },
        escape(&e.id),
        escape(&e.from),
        escape(&e.to),
        path_d(&e.points),
        num(e.width)
    );
}

fn write_edge_label(out: &mut String, e: &SceneEdge) {
    if let Some(l) = &e.label {
        let _ = writeln!(
            out,
            "<text class=\"edge-label role-{}\" data-for=\"{}\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"middle\">{}</text>",
            role_class(e.role),
            escape(&e.id),
            num(l.anchor[0]),
            num(l.anchor[1]),
            escape(&l.text)
        );
    }
}

/// The diagram alone, as an `<svg>` element with an embedded stylesheet.
pub fn render_svg(scene: &Scene) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" id=\"diagram\" class=\"diagram\" viewBox=\"0 0 {} {}\" width=\"100%\" role=\"img\" aria-label=\"diagram\">",
        num(scene.view_w),
        num(scene.view_h)
    );
    out.push_str("<style>");
    out.push_str(SVG_CSS);
    out.push_str("</style>\n<defs>\n");
    for role in ["main", "branch", "async", "return", "error"] {
        let _ = writeln!(
            out,
            "<marker id=\"arrow-{role}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path class=\"arrow role-{role}\" d=\"M0 0.5 L10 5 L0 9.5 z\"/></marker>"
        );
    }
    out.push_str("</defs>\n<g class=\"layer boxes\">\n");
    for b in &scene.boxes {
        let r = b.rect;
        let _ = writeln!(
            out,
            "<g class=\"box {} variant-{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"12\"/><text x=\"{}\" y=\"{}\" dominant-baseline=\"middle\"{}>{}</text></g>",
            box_class(b.kind),
            b.variant.css(),
            num(r.x),
            num(r.y),
            num(r.w),
            num(r.h),
            num(if b.kind == BoxKind::Phase { r.cx() } else { r.x + 12.0 }),
            num(if b.kind == BoxKind::Phase { r.cy() } else { r.y + 13.0 }),
            if b.kind == BoxKind::Phase { " text-anchor=\"middle\"" } else { "" },
            escape(&b.label)
        );
    }
    out.push_str("</g>\n<g class=\"layer lines\">\n");
    for l in &scene.lines {
        let _ = writeln!(
            out,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" class=\"lifeline{}\"/>",
            num(l.a[0]),
            num(l.a[1]),
            num(l.b[0]),
            num(l.b[1]),
            if l.dashed { " dashed" } else { "" }
        );
    }
    out.push_str("</g>\n<g class=\"layer bars\">\n");
    for b in &scene.bars {
        let r = b.rect;
        let _ = writeln!(
            out,
            "<rect class=\"bar kind-{}\" data-owner=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\"/>",
            b.kind.css(),
            escape(&b.owner),
            num(r.x),
            num(r.y),
            num(r.w),
            num(r.h)
        );
    }
    out.push_str("</g>\n<g class=\"layer edges\">\n");
    for e in &scene.edges {
        write_edge(&mut out, e);
    }
    out.push_str("</g>\n<g class=\"layer nodes\">\n");
    for n in &scene.nodes {
        write_node(&mut out, n);
    }
    out.push_str("</g>\n<g class=\"layer labels\">\n");
    for e in &scene.edges {
        write_edge_label(&mut out, e);
    }
    out.push_str("</g>\n</svg>\n");
    out
}

/// The complete standalone page.
pub fn render_html(spec: &Spec, scene: &Scene) -> String {
    let meta = spec.meta();
    let lang = match meta.locale.as_deref() {
        Some("zh-CN") => "zh-CN",
        _ => "en",
    };
    let mut html = String::new();
    let _ = write!(
        html,
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"UTF-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n<meta name=\"generator\" content=\"{GENERATOR}\">\n<title>{}</title>\n<script>(function(){{try{{var t=localStorage.getItem('archify-rs-theme');if(t==='light'||t==='dark'){{document.documentElement.setAttribute('data-theme',t);}}}}catch(e){{}}}})();</script>\n<style>{PAGE_CSS}</style>\n</head>\n<body>\n",
        escape(&meta.title)
    );
    let _ = writeln!(
        html,
        "<header class=\"topbar\"><div class=\"title-block\"><span class=\"dot\"></span><h1>{}</h1>{}</div><div class=\"controls\"><button id=\"theme\" type=\"button\" title=\"Cycle theme\">Theme: system</button><button id=\"reset\" type=\"button\">Reset view</button><button id=\"export-svg\" type=\"button\">Export SVG</button><button id=\"export-png\" type=\"button\">Export PNG</button></div></header>",
        escape(&meta.title),
        meta.subtitle.as_ref().map(|s| format!("<p class=\"subtitle\">{}</p>", escape(s))).unwrap_or_default()
    );
    if !meta.views.is_empty() {
        html.push_str("<nav class=\"views\" aria-label=\"Guided views\"><span class=\"views-title\">Guided views</span>");
        for (i, v) in meta.views.iter().enumerate() {
            let focus = v.focus.join(" ");
            let _ = write!(
                html,
                "<button type=\"button\" class=\"view\" data-focus=\"{}\" title=\"{}\"><span class=\"n\">{:02}</span>{}</button>",
                escape(&focus),
                escape(v.note.as_deref().unwrap_or("")),
                i + 1,
                escape(&v.label)
            );
        }
        html.push_str("<button type=\"button\" class=\"view clear\" data-focus=\"\">Show all</button><span id=\"view-note\" class=\"view-note\"></span></nav>\n");
    }
    html.push_str("<main class=\"stage\" id=\"stage\">\n");
    html.push_str(&render_svg(scene));
    html.push_str("</main>\n");

    // Legend: component kinds present plus the relationship styles.
    html.push_str("<section class=\"legend\"><span class=\"legend-title\">Legend</span>");
    for (kind, count) in scene.kinds_present() {
        let _ = write!(
            html,
            "<span class=\"legend-item\"><i class=\"swatch kind-{}\"></i>{} <b>{}</b></span>",
            kind.css(),
            kind.legend_label(),
            count
        );
    }
    let roles: Vec<EdgeRole> = {
        let mut r: Vec<EdgeRole> = Vec::new();
        for e in &scene.edges {
            if !r.contains(&e.role) {
                r.push(e.role);
            }
        }
        r
    };
    for role in roles {
        let (name, dashed) = match role {
            EdgeRole::Main => ("main path", false),
            EdgeRole::Branch => ("relationship", false),
            EdgeRole::Async => ("async / dashed", true),
            EdgeRole::Return => ("return", true),
            EdgeRole::Error => ("security / exception", false),
        };
        let _ = write!(
            html,
            "<span class=\"legend-item\"><svg width=\"34\" height=\"10\" aria-hidden=\"true\"><line x1=\"1\" y1=\"5\" x2=\"33\" y2=\"5\" class=\"legend-line role-{}\"{}/></svg>{}</span>",
            role_class(role),
            if dashed { " stroke-dasharray=\"5 4\"" } else { "" },
            name
        );
    }
    html.push_str("</section>\n");

    let cards = spec.cards();
    if !cards.is_empty() {
        html.push_str("<section class=\"cards\">");
        for c in cards {
            let _ = write!(
                html,
                "<article class=\"card\"><h2><i class=\"card-dot dot-{}\"></i>{}</h2><ul>",
                escape(&c.dot),
                escape(&c.title)
            );
            for item in &c.items {
                let _ = write!(html, "<li>{}</li>", escape(item));
            }
            html.push_str("</ul></article>");
        }
        html.push_str("</section>\n");
    }
    let _ = write!(
        html,
        "<footer class=\"foot\">{} · {} nodes · {} relationships · viewBox {}×{}</footer>\n<script>{VIEWER_JS}</script>\n</body>\n</html>\n",
        GENERATOR,
        scene.nodes.len(),
        scene.edges.len(),
        num(scene.view_w),
        num(scene.view_h)
    );
    html
}

/// Sanity helper for tests: bounding box of every node.
pub fn node_bounds(scene: &Scene) -> Option<Rect> {
    scene
        .nodes
        .iter()
        .map(|n| n.rect)
        .reduce(|a, b| a.union(&b))
}

const SVG_CSS: &str = r#"
.diagram { font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
.box rect { fill: var(--box-fill); stroke: var(--box-stroke); stroke-width: 1.2; stroke-dasharray: 6 5; }
.box text { font-size: 10px; fill: var(--box-text); letter-spacing: .02em; }
.box.region rect { fill: var(--region-fill); stroke: var(--region-stroke); }
.box.region text { fill: var(--region-stroke); }
.box.security-group rect { fill: var(--sec-fill); stroke: var(--sec-stroke); }
.box.security-group text { fill: var(--sec-stroke); }
.box.lane rect { fill: var(--lane-fill); stroke: var(--lane-stroke); stroke-dasharray: 4 4; }
.box.lane text { fill: var(--muted); font-size: 11px; font-weight: 600; }
.box.lane.exception rect { fill: var(--sec-fill); stroke: var(--sec-stroke); }
.box.lane.exception text { fill: var(--sec-stroke); }
.box.group rect { fill: transparent; stroke: var(--muted); stroke-dasharray: 3 3; }
.box.group.variant-emphasis rect { stroke: var(--edge-main); }
.box.group.variant-security rect { stroke: var(--sec-stroke); }
.box.phase rect { fill: var(--phase-fill); stroke: none; }
.box.phase text { fill: var(--muted); font-size: 10px; font-weight: 600; }
.box.phase.variant-emphasis text { fill: var(--edge-main); }
.box.phase.variant-dashed text { fill: var(--dot-orange); }
.box.band rect { fill: var(--band-fill); stroke: var(--lane-stroke); stroke-dasharray: 4 4; }
.box.band text { fill: var(--muted); font-size: 9.5px; }
.lifeline { stroke: var(--lane-stroke); stroke-width: 1.2; }
.lifeline.dashed { stroke-dasharray: 3 4; }
.bar { stroke-width: 1; }
.edge { fill: none; stroke: var(--edge); stroke-linejoin: round; stroke-linecap: round; }
.edge.dashed { stroke-dasharray: 6 5; }
.edge.role-main { stroke: var(--edge-main); }
.edge.role-async { stroke: var(--edge-async); }
.edge.role-return { stroke: var(--edge); opacity: .85; }
.edge.role-error { stroke: var(--edge-error); }
.arrow { fill: var(--edge); }
.arrow.role-main { fill: var(--edge-main); }
.arrow.role-async { fill: var(--edge-async); }
.arrow.role-error { fill: var(--edge-error); }
.edge-label { font-size: 11px; fill: var(--text); paint-order: stroke; stroke: var(--panel); stroke-width: 4px; stroke-linejoin: round; }
.edge-label.role-main { fill: var(--edge-main); }
.edge-label.role-error { fill: var(--edge-error); }
.edge-label.role-async { fill: var(--edge-async); }
.node > rect { fill: var(--node-fill); stroke: var(--node-stroke); stroke-width: 1.4; }
.node .accent { stroke: none; fill: var(--node-stroke); }
.node .label { fill: var(--text); font-weight: 650; }
.node .sublabel { fill: var(--muted); }
.node .tag rect { fill: var(--panel); stroke: var(--node-stroke); stroke-width: 1; }
.node .tag text { fill: var(--node-stroke); font-weight: 600; }
.kind-frontend { --node-fill: var(--frontend-fill); --node-stroke: var(--frontend); }
.kind-backend { --node-fill: var(--backend-fill); --node-stroke: var(--backend); }
.kind-database { --node-fill: var(--database-fill); --node-stroke: var(--database); }
.kind-cloud { --node-fill: var(--cloud-fill); --node-stroke: var(--cloud); }
.kind-security { --node-fill: var(--security-fill); --node-stroke: var(--security); }
.kind-messagebus { --node-fill: var(--messagebus-fill); --node-stroke: var(--messagebus); }
.kind-external { --node-fill: var(--external-fill); --node-stroke: var(--external); }
.bar.kind-frontend { fill: var(--frontend-fill); stroke: var(--frontend); }
.bar.kind-backend { fill: var(--backend-fill); stroke: var(--backend); }
.bar.kind-database { fill: var(--database-fill); stroke: var(--database); }
.bar.kind-cloud { fill: var(--cloud-fill); stroke: var(--cloud); }
.bar.kind-security { fill: var(--security-fill); stroke: var(--security); }
.bar.kind-messagebus { fill: var(--messagebus-fill); stroke: var(--messagebus); }
.bar.kind-external { fill: var(--external-fill); stroke: var(--external); }
.dim { opacity: .18; transition: opacity .2s; }
"#;

const PAGE_CSS: &str = r#"
:root { color-scheme: light;
  --bg:#f5f7fa; --panel:#ffffff; --text:#111827; --muted:#5b6472; --border:#d9dee7;
  --edge:#6b7280; --edge-main:#0f9f6e; --edge-async:#7c3aed; --edge-error:#dc2626;
  --box-fill:transparent; --box-stroke:#9aa3b2; --box-text:#5b6472;
  --region-fill:rgba(245,158,11,.05); --region-stroke:#d97706; --sec-fill:rgba(220,38,38,.05); --sec-stroke:#dc2626;
  --lane-fill:rgba(148,163,184,.08); --lane-stroke:#b6bfcc; --phase-fill:rgba(148,163,184,.14); --band-fill:rgba(148,163,184,.06);
  --frontend:#0891b2; --frontend-fill:#e0f7fb; --backend:#059669; --backend-fill:#dcf5ea; --database:#7c3aed; --database-fill:#ede9fe;
  --cloud:#d97706; --cloud-fill:#fef3c7; --security:#dc2626; --security-fill:#fee2e2; --messagebus:#ea580c; --messagebus-fill:#ffedd5; --external:#475569; --external-fill:#e8ecf2;
  --dot-cyan:#0891b2; --dot-emerald:#059669; --dot-violet:#7c3aed; --dot-amber:#d97706; --dot-rose:#e11d48; --dot-orange:#ea580c; --dot-slate:#475569; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { color-scheme: dark;
  --bg:#0b0f17; --panel:#111827; --text:#e5e7eb; --muted:#9aa4b2; --border:#26303f;
  --edge:#94a3b8; --edge-main:#34d399; --edge-async:#a78bfa; --edge-error:#f87171;
  --box-stroke:#4b5563; --box-text:#9aa4b2; --region-fill:rgba(245,158,11,.06); --region-stroke:#f59e0b; --sec-fill:rgba(248,113,113,.07); --sec-stroke:#f87171;
  --lane-fill:rgba(148,163,184,.06); --lane-stroke:#334155; --phase-fill:rgba(148,163,184,.12); --band-fill:rgba(148,163,184,.05);
  --frontend:#22d3ee; --frontend-fill:#0e2f38; --backend:#34d399; --backend-fill:#0d2f25; --database:#a78bfa; --database-fill:#231a44;
  --cloud:#fbbf24; --cloud-fill:#3a2a0c; --security:#f87171; --security-fill:#3b1414; --messagebus:#fb923c; --messagebus-fill:#3b2110; --external:#cbd5e1; --external-fill:#1f2937;
  --dot-cyan:#22d3ee; --dot-emerald:#34d399; --dot-violet:#a78bfa; --dot-amber:#fbbf24; --dot-rose:#fb7185; --dot-orange:#fb923c; --dot-slate:#cbd5e1; } }
:root[data-theme="dark"] { color-scheme: dark;
  --bg:#0b0f17; --panel:#111827; --text:#e5e7eb; --muted:#9aa4b2; --border:#26303f;
  --edge:#94a3b8; --edge-main:#34d399; --edge-async:#a78bfa; --edge-error:#f87171;
  --box-stroke:#4b5563; --box-text:#9aa4b2; --region-fill:rgba(245,158,11,.06); --region-stroke:#f59e0b; --sec-fill:rgba(248,113,113,.07); --sec-stroke:#f87171;
  --lane-fill:rgba(148,163,184,.06); --lane-stroke:#334155; --phase-fill:rgba(148,163,184,.12); --band-fill:rgba(148,163,184,.05);
  --frontend:#22d3ee; --frontend-fill:#0e2f38; --backend:#34d399; --backend-fill:#0d2f25; --database:#a78bfa; --database-fill:#231a44;
  --cloud:#fbbf24; --cloud-fill:#3a2a0c; --security:#f87171; --security-fill:#3b1414; --messagebus:#fb923c; --messagebus-fill:#3b2110; --external:#cbd5e1; --external-fill:#1f2937;
  --dot-cyan:#22d3ee; --dot-emerald:#34d399; --dot-violet:#a78bfa; --dot-amber:#fbbf24; --dot-rose:#fb7185; --dot-orange:#fb923c; --dot-slate:#cbd5e1; }
* { box-sizing: border-box; }
html, body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
body { padding: 18px 24px 28px; max-width: 1600px; margin: 0 auto; }
.topbar { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; justify-content: space-between; margin-bottom: 12px; }
.title-block { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
.title-block h1 { font-size: 22px; margin: 0; letter-spacing: -.01em; }
.title-block .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--edge-main); display: inline-block; align-self: center; }
.subtitle { margin: 0; color: var(--muted); }
.controls { display: flex; gap: 8px; flex-wrap: wrap; }
button { font: inherit; font-size: 12.5px; padding: 6px 11px; border-radius: 8px; border: 1px solid var(--border); background: var(--panel); color: var(--text); cursor: pointer; }
button:hover { border-color: var(--edge-main); }
.views { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 12px; padding: 8px 12px; border: 1px solid var(--border); border-radius: 12px; background: var(--panel); }
.views-title { font-size: 10.5px; letter-spacing: .12em; text-transform: uppercase; color: var(--muted); margin-right: 4px; }
.view .n { display: inline-block; font-size: 10px; margin-right: 6px; color: var(--edge-main); }
.view.active { border-color: var(--edge-main); box-shadow: 0 0 0 2px color-mix(in srgb, var(--edge-main) 25%, transparent); }
.view-note { color: var(--muted); font-size: 12.5px; }
.stage { background: var(--panel); border: 1px solid var(--border); border-radius: 14px; padding: 12px; overflow: hidden; cursor: grab; }
.stage.dragging { cursor: grabbing; }
.stage svg { display: block; width: 100%; height: auto; max-height: calc(100vh - 220px); }
.legend { display: flex; flex-wrap: wrap; gap: 14px; align-items: center; margin: 12px 4px 0; color: var(--muted); font-size: 12.5px; }
.legend-title { font-weight: 650; color: var(--text); }
.legend-item { display: inline-flex; align-items: center; gap: 6px; }
.legend-item b { font-weight: 600; background: var(--phase-fill); border-radius: 6px; padding: 0 5px; font-size: 11px; }
.swatch { width: 14px; height: 10px; border-radius: 3px; border: 1.5px solid var(--node-stroke); background: var(--node-fill); display: inline-block; }
.legend-line { stroke: var(--edge); stroke-width: 2; }
.legend-line.role-main { stroke: var(--edge-main); }
.legend-line.role-async { stroke: var(--edge-async); }
.legend-line.role-error { stroke: var(--edge-error); }
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 12px; margin-top: 16px; }
.card { background: var(--panel); border: 1px solid var(--border); border-radius: 12px; padding: 14px 16px; }
.card h2 { font-size: 14px; margin: 0 0 8px; display: flex; align-items: center; gap: 8px; }
.card ul { margin: 0; padding-left: 16px; color: var(--muted); font-size: 13px; }
.card li + li { margin-top: 4px; }
.card-dot { width: 9px; height: 9px; border-radius: 50%; display: inline-block; background: var(--dot-slate); }
.dot-cyan { background: var(--dot-cyan); } .dot-emerald { background: var(--dot-emerald); } .dot-violet { background: var(--dot-violet); }
.dot-amber { background: var(--dot-amber); } .dot-rose { background: var(--dot-rose); } .dot-orange { background: var(--dot-orange); } .dot-slate { background: var(--dot-slate); }
.foot { margin-top: 14px; color: var(--muted); font-size: 11.5px; }
@media (max-width: 640px) { body { padding: 12px 16px 20px; } .title-block h1 { font-size: 18px; } }
"#;

const VIEWER_JS: &str = r#"
(function () {
  var svg = document.getElementById('diagram');
  var stage = document.getElementById('stage');
  var base = svg.getAttribute('viewBox').split(' ').map(Number);
  var vb = base.slice();
  function apply() { svg.setAttribute('viewBox', vb.join(' ')); }
  function reset() { vb = base.slice(); apply(); }
  document.getElementById('reset').addEventListener('click', reset);

  // Pan and zoom by editing the viewBox; the authored geometry never changes.
  stage.addEventListener('wheel', function (ev) {
    ev.preventDefault();
    var rect = svg.getBoundingClientRect();
    var px = (ev.clientX - rect.left) / rect.width, py = (ev.clientY - rect.top) / rect.height;
    var k = ev.deltaY < 0 ? 0.9 : 1.1;
    var nw = Math.min(Math.max(vb[2] * k, base[2] / 8), base[2] * 3), nh = nw * (base[3] / base[2]);
    vb[0] += (vb[2] - nw) * px; vb[1] += (vb[3] - nh) * py; vb[2] = nw; vb[3] = nh; apply();
  }, { passive: false });
  var drag = null;
  stage.addEventListener('pointerdown', function (ev) { drag = { x: ev.clientX, y: ev.clientY, vb: vb.slice() }; stage.classList.add('dragging'); stage.setPointerCapture(ev.pointerId); });
  stage.addEventListener('pointermove', function (ev) {
    if (!drag) return;
    var rect = svg.getBoundingClientRect();
    vb[0] = drag.vb[0] - (ev.clientX - drag.x) * (vb[2] / rect.width);
    vb[1] = drag.vb[1] - (ev.clientY - drag.y) * (vb[3] / rect.height);
    apply();
  });
  function endDrag() { drag = null; stage.classList.remove('dragging'); }
  stage.addEventListener('pointerup', endDrag); stage.addEventListener('pointercancel', endDrag);
  stage.addEventListener('dblclick', reset);

  // Theme: system -> light -> dark, remembered per browser.
  var themeBtn = document.getElementById('theme');
  function themeLabel() { var t = document.documentElement.getAttribute('data-theme'); themeBtn.textContent = 'Theme: ' + (t || 'system'); }
  themeBtn.addEventListener('click', function () {
    var t = document.documentElement.getAttribute('data-theme');
    var next = !t ? 'light' : (t === 'light' ? 'dark' : null);
    if (next) document.documentElement.setAttribute('data-theme', next); else document.documentElement.removeAttribute('data-theme');
    try { if (next) localStorage.setItem('archify-rs-theme', next); else localStorage.removeItem('archify-rs-theme'); } catch (e) {}
    themeLabel();
  });
  themeLabel();

  // Guided views: dim everything outside the focus set.
  var note = document.getElementById('view-note');
  var viewButtons = Array.prototype.slice.call(document.querySelectorAll('.view'));
  function focus(ids, btn) {
    var set = {}; ids.forEach(function (id) { set[id] = true; });
    var any = ids.length > 0;
    svg.querySelectorAll('.node').forEach(function (n) { n.classList.toggle('dim', any && !set[n.getAttribute('data-id')]); });
    svg.querySelectorAll('.edge').forEach(function (e) { e.classList.toggle('dim', any && !(set[e.getAttribute('data-from')] && set[e.getAttribute('data-to')])); });
    svg.querySelectorAll('.edge-label').forEach(function (l) {
      var e = svg.querySelector('.edge[data-id="' + l.getAttribute('data-for').replace(/"/g, '\\"') + '"]');
      l.classList.toggle('dim', !!(e && e.classList.contains('dim')));
    });
    svg.querySelectorAll('.bar').forEach(function (b) { b.classList.toggle('dim', any && !set[b.getAttribute('data-owner')]); });
    viewButtons.forEach(function (b) { b.classList.toggle('active', b === btn); });
    if (note) note.textContent = btn && btn.getAttribute('title') ? btn.getAttribute('title') : '';
  }
  viewButtons.forEach(function (b) {
    b.addEventListener('click', function () {
      var ids = b.getAttribute('data-focus').split(' ').filter(Boolean);
      focus(b.classList.contains('active') ? [] : ids, b.classList.contains('active') ? null : b);
    });
  });

  // Export: clone the SVG, resolve every CSS variable to its current value so
  // the file stands alone, and hand the bytes to the browser.
  function resolvedSvg() {
    var clone = svg.cloneNode(true);
    clone.setAttribute('viewBox', base.join(' '));
    clone.removeAttribute('width');
    clone.setAttribute('width', base[2]); clone.setAttribute('height', base[3]);
    var cs = getComputedStyle(document.documentElement);
    var css = clone.querySelector('style').textContent.replace(/var\((--[a-z0-9-]+)\)/g, function (m, name) {
      var v = cs.getPropertyValue(name).trim();
      return v || m;
    });
    // Kind classes alias --node-fill/--node-stroke; expand those aliases too.
    var kinds = ['frontend','backend','database','cloud','security','messagebus','external'];
    kinds.forEach(function (k) {
      var stroke = cs.getPropertyValue('--' + k).trim(), fill = cs.getPropertyValue('--' + k + '-fill').trim();
      css += '\n.kind-' + k + ' > rect{fill:' + fill + ';stroke:' + stroke + '}.kind-' + k + ' .accent{fill:' + stroke + '}.kind-' + k + ' .tag rect{stroke:' + stroke + '}.kind-' + k + ' .tag text{fill:' + stroke + '}';
    });
    css += '\n.edge-label{stroke:' + cs.getPropertyValue('--panel').trim() + '}';
    clone.querySelector('style').textContent = css + '\nsvg{background:' + cs.getPropertyValue('--panel').trim() + '}';
    clone.querySelectorAll('.dim').forEach(function (el) { el.classList.remove('dim'); });
    return new XMLSerializer().serializeToString(clone);
  }
  function download(blob, name) {
    var a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = name; document.body.appendChild(a); a.click();
    setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 500);
  }
  var slug = (document.title || 'diagram').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  document.getElementById('export-svg').addEventListener('click', function () {
    download(new Blob([resolvedSvg()], { type: 'image/svg+xml' }), slug + '.svg');
  });
  document.getElementById('export-png').addEventListener('click', function () {
    var img = new Image();
    var url = URL.createObjectURL(new Blob([resolvedSvg()], { type: 'image/svg+xml' }));
    img.onload = function () {
      var scale = 2, c = document.createElement('canvas');
      c.width = base[2] * scale; c.height = base[3] * scale;
      var ctx = c.getContext('2d');
      ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--panel').trim() || '#fff';
      ctx.fillRect(0, 0, c.width, c.height);
      ctx.drawImage(img, 0, 0, c.width, c.height);
      URL.revokeObjectURL(url);
      c.toBlob(function (b) { if (b) download(b, slug + '.png'); }, 'image/png');
    };
    img.src = url;
  });
})();
"#;
