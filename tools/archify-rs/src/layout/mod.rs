//! Diagram-type layouts: each turns a parsed spec into a [`Scene`].

pub mod architecture;
pub mod sequence;
pub mod workflow;

use std::collections::HashMap;

use crate::diag::Diagnostic;
use crate::geom::{self, Rect};
use crate::scene::{EdgeRole, Scene, SceneEdge, SceneLabel, LABEL_FONT_PX, LABEL_H};
use crate::spec::{Point, Side, Spec, Variant};

pub fn build(spec: &Spec) -> Result<Scene, Vec<Diagnostic>> {
    match spec {
        Spec::Architecture(a) => architecture::layout(a),
        Spec::Sequence(s) => sequence::layout(s),
        Spec::Workflow(w) => workflow::layout(w),
    }
}

/// Everything the shared router needs to know about one relationship.
pub struct EdgeInput {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub variant: Variant,
    pub role: EdgeRole,
    pub from_side: Option<Side>,
    pub to_side: Option<Side>,
    pub via: Vec<Point>,
    pub label_at: Option<Point>,
    pub label_dx: f64,
    pub label_dy: f64,
    pub label_segment: Option<usize>,
    pub width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PortKey<'a> {
    node: &'a str,
    side: Side,
}

/// Route a batch of relationships against a set of node rectangles, spreading
/// ports that share a side so parallel relationships stay distinct.
pub fn route_edges(
    rects: &HashMap<String, Rect>,
    inputs: &[EdgeInput],
    what: &str,
) -> Result<Vec<SceneEdge>, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    for e in inputs {
        for end in [&e.from, &e.to] {
            if !rects.contains_key(end.as_str()) {
                errors.push(Diagnostic::error(
                    "references/unknown-id",
                    format!("{what} \"{}\" references unknown id \"{end}\"", e.id),
                ));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Resolve the side contract for every endpoint first.
    let sides: Vec<(Side, Side)> = inputs
        .iter()
        .map(|e| {
            let (fs, ts) = geom::infer_sides(&rects[&e.from], &rects[&e.to]);
            (e.from_side.unwrap_or(fs), e.to_side.unwrap_or(ts))
        })
        .collect();

    // Group endpoints per (node, side) and order them by the far endpoint so
    // ports do not cross each other where they leave the node.
    let mut groups: HashMap<PortKey, Vec<(usize, bool, f64)>> = HashMap::new();
    for (i, e) in inputs.iter().enumerate() {
        let (fs, ts) = sides[i];
        let far_to = rects[&e.to].center();
        let far_from = rects[&e.from].center();
        let key_along = |side: Side, far: Point| if side.is_horizontal() { far[1] } else { far[0] };
        groups
            .entry(PortKey {
                node: &e.from,
                side: fs,
            })
            .or_default()
            .push((i, true, key_along(fs, far_to)));
        groups
            .entry(PortKey {
                node: &e.to,
                side: ts,
            })
            .or_default()
            .push((i, false, key_along(ts, far_from)));
    }
    let mut fracs: Vec<(f64, f64)> = vec![(0.5, 0.5); inputs.len()];
    let mut spread_from = vec![false; inputs.len()];
    let mut spread_to = vec![false; inputs.len()];
    for (_, mut members) in groups {
        if members.len() == 1 {
            continue;
        }
        members.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        let n = members.len() as f64;
        for (slot, (idx, is_from, _)) in members.into_iter().enumerate() {
            let frac = (slot as f64 + 1.0) / (n + 1.0);
            if is_from {
                fracs[idx].0 = frac;
                spread_from[idx] = true;
            } else {
                fracs[idx].1 = frac;
                spread_to[idx] = true;
            }
        }
    }

    let content_right = rects.values().map(|r| r.right()).fold(0.0_f64, f64::max);
    let edges = inputs
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (fs, ts) = sides[i];
            let mut start = geom::port(&rects[&e.from], fs, fracs[i].0);
            let mut end = geom::port(&rects[&e.to], ts, fracs[i].1);
            // When exactly one endpoint was spread and the sides face each
            // other, slide the unshared endpoint onto the spread port's axis
            // so the run stays straight instead of jogging.
            if e.via.is_empty() && spread_from[i] != spread_to[i] {
                let facing_vertical = !fs.is_horizontal() && !ts.is_horizontal();
                let facing_horizontal = fs.is_horizontal() && ts.is_horizontal();
                const CORNER: f64 = 12.0;
                if spread_from[i] {
                    let tr = rects[&e.to];
                    if facing_vertical
                        && start[0] >= tr.x + CORNER
                        && start[0] <= tr.right() - CORNER
                    {
                        end[0] = start[0];
                    } else if facing_horizontal
                        && start[1] >= tr.y + CORNER
                        && start[1] <= tr.bottom() - CORNER
                    {
                        end[1] = start[1];
                    }
                } else {
                    let fr = rects[&e.from];
                    if facing_vertical && end[0] >= fr.x + CORNER && end[0] <= fr.right() - CORNER {
                        start[0] = end[0];
                    } else if facing_horizontal
                        && end[1] >= fr.y + CORNER
                        && end[1] <= fr.bottom() - CORNER
                    {
                        start[1] = end[1];
                    }
                }
            }
            let points = geom::route(start, fs, end, ts, &e.via);
            let label = e
                .label
                .as_ref()
                .filter(|l| !l.trim().is_empty())
                .map(|text| {
                    place_label_within(
                        &points,
                        text,
                        e.label_segment,
                        e.label_at,
                        e.label_dx,
                        e.label_dy,
                        content_right,
                    )
                });
            SceneEdge {
                id: e.id.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                points,
                label,
                variant: e.variant,
                role: e.role,
                width: e.width,
            }
        })
        .collect();
    Ok(edges)
}

pub fn place_label(
    points: &[Point],
    text: &str,
    segment: Option<usize>,
    at: Option<Point>,
    dx: f64,
    dy: f64,
) -> SceneLabel {
    place_label_within(points, text, segment, at, dx, dy, f64::INFINITY)
}

/// Labels on horizontal runs float just above the line; labels on vertical
/// runs sit beside it, on the right unless that would push past the right
/// edge of the node field, in which case they flip to the left.
pub fn place_label_within(
    points: &[Point],
    text: &str,
    segment: Option<usize>,
    at: Option<Point>,
    dx: f64,
    dy: f64,
    content_right: f64,
) -> SceneLabel {
    let segs = geom::segments(points);
    let idx = segment
        .filter(|i| *i < segs.len())
        .unwrap_or_else(|| geom::longest_segment(points));
    let seg = segs
        .get(idx)
        .copied()
        .unwrap_or(geom::Segment::new(points[0], points[0]));
    let horizontal = seg.is_horizontal();
    let mid = seg.midpoint();
    let w = geom::text_width(text, LABEL_FONT_PX);
    let mut anchor = at.unwrap_or(if horizontal {
        [mid[0], mid[1] - (LABEL_H / 2.0 + 4.0)]
    } else if mid[0] + 6.0 + w <= content_right + 16.0 {
        [mid[0] + 6.0 + w / 2.0, mid[1]]
    } else {
        [mid[0] - 6.0 - w / 2.0, mid[1]]
    });
    anchor[0] += dx;
    anchor[1] += dy;
    SceneLabel {
        text: text.to_string(),
        rect: Rect::from_center(anchor, w, LABEL_H),
        anchor,
        horizontal,
    }
}

pub fn role_from(role: Option<&str>, variant: Variant) -> EdgeRole {
    match role {
        Some("main") => EdgeRole::Main,
        Some("branch") => EdgeRole::Branch,
        Some("async") => EdgeRole::Async,
        Some("return") => EdgeRole::Return,
        Some("error") => EdgeRole::Error,
        _ => match variant {
            Variant::Emphasis => EdgeRole::Main,
            Variant::Dashed => EdgeRole::Async,
            Variant::Return => EdgeRole::Return,
            Variant::Security => EdgeRole::Error,
            Variant::Default => EdgeRole::Branch,
        },
    }
}

pub fn edge_id(explicit: Option<&str>, from: &str, to: &str, index: usize) -> String {
    explicit
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{from}-to-{to}-{index}"))
}
