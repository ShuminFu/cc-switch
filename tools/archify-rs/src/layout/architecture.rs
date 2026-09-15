use std::collections::HashMap;

use super::{edge_id, role_from, route_edges, EdgeInput};
use crate::diag::Diagnostic;
use crate::geom::Rect;
use crate::scene::{BoxKind, Scene, SceneBox, SceneNode, NODE_LABEL_PX, NODE_SUBLABEL_PX};
use crate::spec::{Architecture, Variant};

const DEFAULT_SIZE: [f64; 2] = [130.0, 60.0];
const MARGIN: f64 = 40.0;

pub fn layout(a: &Architecture) -> Result<Scene, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut rects: HashMap<String, Rect> = HashMap::new();
    let mut scene = Scene::default();

    let grid = a.layout.as_ref();
    let origin = grid.and_then(|g| g.origin).unwrap_or([MARGIN, MARGIN]);
    let cell_w = grid.and_then(|g| g.cell_w).unwrap_or(DEFAULT_SIZE[0]);
    let cell_h = grid.and_then(|g| g.cell_h).unwrap_or(DEFAULT_SIZE[1]);
    let gap_x = grid.and_then(|g| g.gap_x).unwrap_or(80.0);
    let gap_y = grid.and_then(|g| g.gap_y).unwrap_or(90.0);

    for c in &a.components {
        let size = c.size.unwrap_or(DEFAULT_SIZE);
        let pos = match (c.pos, c.row, c.col) {
            (Some(p), _, _) => p,
            (None, Some(r), Some(col)) => [
                origin[0] + col as f64 * (cell_w + gap_x),
                origin[1] + r as f64 * (cell_h + gap_y),
            ],
            _ => {
                errors.push(Diagnostic::error(
                    "layout/unplaced-component",
                    format!("component \"{}\" needs either pos or row+col", c.id),
                ));
                continue;
            }
        };
        let rect = Rect::new(pos[0], pos[1], size[0], size[1]);
        rects.insert(c.id.clone(), rect);
        scene.nodes.push(SceneNode {
            id: c.id.clone(),
            rect,
            kind: c.kind,
            label: c.label.clone(),
            sublabel: c.sublabel.clone(),
            tag: c.tag.clone(),
            label_px: NODE_LABEL_PX,
            sublabel_px: NODE_SUBLABEL_PX,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    for (i, b) in a.boundaries.iter().enumerate() {
        let (kind, default_pad) = match b.kind.as_str() {
            "region" => (BoxKind::Region, 30.0),
            "security-group" => (BoxKind::SecurityGroup, 18.0),
            other => {
                errors.push(Diagnostic::error(
                    "spec/boundary-kind",
                    format!("boundary \"{}\" has unknown kind \"{other}\"", b.label),
                ));
                continue;
            }
        };
        let mut bbox: Option<Rect> = None;
        for id in &b.wraps {
            match rects.get(id) {
                Some(r) => bbox = Some(bbox.map_or(*r, |acc| acc.union(r))),
                None => errors.push(Diagnostic::error(
                    "references/unknown-id",
                    format!("boundary \"{}\" wraps unknown component \"{id}\"", b.label),
                )),
            }
        }
        if let Some(bb) = bbox {
            let pad = b.pad.unwrap_or(default_pad);
            let mut rect = bb.expanded(pad);
            // Leave headroom for the boundary caption.
            rect.y -= 12.0;
            rect.h += 12.0;
            scene.boxes.push(SceneBox {
                id: format!("boundary-{i}"),
                kind,
                rect,
                label: b.label.clone(),
                variant: Variant::Default,
            });
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let inputs: Vec<EdgeInput> = a
        .connections
        .iter()
        .enumerate()
        .map(|(i, c)| EdgeInput {
            id: edge_id(c.id.as_deref(), &c.from, &c.to, i),
            from: c.from.clone(),
            to: c.to.clone(),
            label: c.label.clone(),
            variant: c.variant,
            role: role_from(None, c.variant),
            from_side: c.from_side,
            to_side: c.to_side,
            via: c.via.clone(),
            label_at: c.label_at,
            label_dx: c.label_dx.unwrap_or(0.0),
            label_dy: c.label_dy.unwrap_or(0.0),
            label_segment: c.label_segment,
            width: c.width.unwrap_or(1.6),
        })
        .collect();
    scene.edges = route_edges(&rects, &inputs, "connection")?;

    // Draw regions before security groups so nested groups stay visible.
    scene
        .boxes
        .sort_by_key(|b| matches!(b.kind, BoxKind::SecurityGroup));

    match a.meta.view_box {
        Some([w, h]) => {
            scene.view_w = w;
            scene.view_h = h;
        }
        None => {
            let bounds = scene
                .content_bounds()
                .unwrap_or(Rect::new(0.0, 0.0, 320.0, 240.0));
            scene.view_w = (bounds.right() + MARGIN).max(320.0).ceil();
            scene.view_h = (bounds.bottom() + MARGIN).max(240.0).ceil();
        }
    }
    Ok(scene)
}
