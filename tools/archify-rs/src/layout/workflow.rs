use std::collections::HashMap;

use super::{edge_id, role_from, route_edges, EdgeInput};
use crate::diag::Diagnostic;
use crate::geom::Rect;
use crate::scene::{BoxKind, Scene, SceneBox, SceneNode, NODE_LABEL_PX, NODE_SUBLABEL_PX};
use crate::spec::{Side, Variant, Workflow};

const DEFAULT_NODE_W: f64 = 92.0;
const DEFAULT_NODE_H: f64 = 56.0;
const COL_GAP: f64 = 56.0;
const LANE_H: f64 = 124.0;
/// Wide enough for a 14px label to sit between lanes with clear margins.
const LANE_GAP: f64 = 30.0;
const LEFT: f64 = 40.0;
const PHASE_BAND_Y: f64 = 22.0;
const PHASE_BAND_H: f64 = 22.0;
const LANES_TOP: f64 = 62.0;
const COLUMNS: usize = 6;

pub fn layout(w: &Workflow) -> Result<Scene, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let lane_index: HashMap<&str, usize> = w
        .lanes
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect();

    // Column widths follow the widest node placed in each column.
    let mut col_w = [DEFAULT_NODE_W; COLUMNS];
    let mut used_cols = 0usize;
    for n in &w.nodes {
        let c = n.col as usize;
        col_w[c] = col_w[c].max(n.width.unwrap_or(DEFAULT_NODE_W));
        used_cols = used_cols.max(c + 1);
    }
    for p in &w.phases {
        used_cols = used_cols.max(p.to_col as usize + 1);
    }
    let mut col_x = [0.0; COLUMNS];
    let mut x = LEFT;
    for c in 0..COLUMNS {
        col_x[c] = x;
        x += col_w[c] + COL_GAP;
    }
    let right_edge = col_x[used_cols - 1] + col_w[used_cols - 1];

    let mut scene = Scene::default();
    let lane_x = LEFT - 16.0;
    let lane_w = right_edge - LEFT + 32.0;
    let mut lane_rects = Vec::new();
    for (i, lane) in w.lanes.iter().enumerate() {
        let rect = Rect::new(
            lane_x,
            LANES_TOP + i as f64 * (LANE_H + LANE_GAP),
            lane_w,
            LANE_H,
        );
        lane_rects.push(rect);
        scene.boxes.push(SceneBox {
            id: format!("lane-{}", lane.id),
            kind: if lane.is_exception() {
                BoxKind::ExceptionLane
            } else {
                BoxKind::Lane
            },
            rect,
            label: if lane.is_exception() {
                format!("EX / {}", lane.label)
            } else {
                format!("{:02} / {}", i + 1, lane.label)
            },
            variant: Variant::Default,
        });
    }

    for p in &w.phases {
        let x0 = col_x[p.from_col as usize];
        let x1 = col_x[p.to_col as usize] + col_w[p.to_col as usize];
        scene.boxes.push(SceneBox {
            id: format!("phase-{}", p.id),
            kind: BoxKind::Phase,
            rect: Rect::new(x0, PHASE_BAND_Y, x1 - x0, PHASE_BAND_H),
            label: p.label.clone(),
            variant: p.variant,
        });
    }

    for g in &w.groups {
        let Some(&li) = lane_index.get(g.lane.as_str()) else {
            errors.push(Diagnostic::error(
                "references/unknown-id",
                format!("group \"{}\" references unknown lane \"{}\"", g.id, g.lane),
            ));
            continue;
        };
        let lane = lane_rects[li];
        let x0 = col_x[g.from_col as usize] - 12.0;
        let x1 = col_x[g.to_col as usize] + col_w[g.to_col as usize] + 12.0;
        scene.boxes.push(SceneBox {
            id: format!("group-{}", g.id),
            kind: BoxKind::Group,
            rect: Rect::new(x0, lane.y + 24.0, x1 - x0, lane.h - 32.0),
            label: g.label.clone(),
            variant: g.variant,
        });
    }

    let mut rects: HashMap<String, Rect> = HashMap::new();
    let mut node_lane: HashMap<&str, usize> = HashMap::new();
    let mut node_col: HashMap<&str, u32> = HashMap::new();
    for n in &w.nodes {
        let Some(&li) = lane_index.get(n.lane.as_str()) else {
            errors.push(Diagnostic::error(
                "references/unknown-id",
                format!("node \"{}\" references unknown lane \"{}\"", n.id, n.lane),
            ));
            continue;
        };
        let lane = lane_rects[li];
        let c = n.col as usize;
        let nw = n.width.unwrap_or(DEFAULT_NODE_W);
        let nh = n.height.unwrap_or(DEFAULT_NODE_H);
        let rect = Rect::new(
            col_x[c] + (col_w[c] - nw) / 2.0,
            lane.y + (lane.h - nh) / 2.0 + 8.0 + n.y_offset.unwrap_or(0.0),
            nw,
            nh,
        );
        rects.insert(n.id.clone(), rect);
        node_lane.insert(&n.id, li);
        node_col.insert(&n.id, n.col);
        scene.nodes.push(SceneNode {
            id: n.id.clone(),
            rect,
            kind: n.kind,
            label: n.label.clone(),
            sublabel: n.sublabel.clone(),
            tag: n.tag.clone(),
            label_px: NODE_LABEL_PX,
            sublabel_px: NODE_SUBLABEL_PX,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut inputs = Vec::with_capacity(w.edges.len());
    for (i, e) in w.edges.iter().enumerate() {
        let id = edge_id(e.id.as_deref(), &e.from, &e.to, i);
        let (Some(&fl), Some(&tl)) = (node_lane.get(e.from.as_str()), node_lane.get(e.to.as_str()))
        else {
            let missing = if node_lane.contains_key(e.from.as_str()) {
                &e.to
            } else {
                &e.from
            };
            errors.push(Diagnostic::error(
                "references/unknown-id",
                format!("edge \"{id}\" references unknown node \"{missing}\""),
            ));
            continue;
        };
        let fc = node_col[e.from.as_str()];
        let tc = node_col[e.to.as_str()];
        let mut via = e.via.clone();
        let (mut fs, mut ts) = if fl == tl {
            if tc > fc + 1 || tc < fc {
                // Skip intermediate nodes through a channel under the lane.
                let channel = e.channel_y.unwrap_or(lane_rects[fl].bottom() - 10.0);
                let fr = rects[&e.from];
                let tr = rects[&e.to];
                via = vec![[fr.cx(), channel], [tr.cx(), channel]];
                (Side::Bottom, Side::Bottom)
            } else if tc > fc {
                (Side::Right, Side::Left)
            } else {
                (Side::Left, Side::Right)
            }
        } else if tl > fl {
            (Side::Bottom, Side::Top)
        } else {
            (Side::Top, Side::Bottom)
        };
        match e.route.as_deref() {
            Some("straight") => {
                fs = if tc >= fc { Side::Right } else { Side::Left };
                ts = if tc >= fc { Side::Left } else { Side::Right };
                via.clear();
            }
            Some("drop") => {
                fs = if tl >= fl { Side::Bottom } else { Side::Top };
                ts = if tl >= fl { Side::Top } else { Side::Bottom };
                via.clear();
            }
            _ => {}
        }
        if let Some(cx) = e.channel_x {
            let fr = rects[&e.from];
            let tr = rects[&e.to];
            via = vec![[cx, fr.cy()], [cx, tr.cy()]];
            fs = if cx >= fr.cx() {
                Side::Right
            } else {
                Side::Left
            };
            ts = if cx >= tr.cx() {
                Side::Right
            } else {
                Side::Left
            };
        }
        inputs.push(EdgeInput {
            id,
            from: e.from.clone(),
            to: e.to.clone(),
            label: e.label.clone(),
            variant: e.variant,
            role: role_from(e.role.as_deref(), e.variant),
            from_side: Some(e.from_side.unwrap_or(fs)),
            to_side: Some(e.to_side.unwrap_or(ts)),
            via,
            label_at: e.label_at,
            label_dx: e.label_dx.unwrap_or(0.0),
            label_dy: e.label_dy.unwrap_or(0.0),
            label_segment: e.label_segment,
            width: e.width.unwrap_or(1.6),
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    scene.edges = route_edges(&rects, &inputs, "edge")?;

    let lanes_bottom = lane_rects.last().map(|r| r.bottom()).unwrap_or(LANES_TOP);
    match w.meta.view_box {
        Some([vw, vh]) => {
            scene.view_w = vw;
            scene.view_h = vh;
        }
        None => {
            scene.view_w = (right_edge + LEFT).ceil();
            scene.view_h = (lanes_bottom + 28.0).ceil();
        }
    }
    Ok(scene)
}
