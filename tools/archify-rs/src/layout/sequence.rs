use std::collections::HashMap;

use super::place_label;
use crate::diag::Diagnostic;
use crate::geom::{self, Rect};
use crate::scene::{
    BoxKind, EdgeRole, Scene, SceneBar, SceneBox, SceneEdge, SceneLine, SceneNode, NODE_LABEL_PX,
    NODE_SUBLABEL_PX,
};
use crate::spec::{Sequence, Variant};

const FIXED_BOX_W: f64 = 86.0;
const FIXED_GAP: f64 = 108.0;
const BOX_H: f64 = 56.0;
const BOX_Y: f64 = 48.0;
const SIDE_MARGIN: f64 = 40.0;
/// Messages must start below the participant header and end above the legend.
pub const TIMELINE_TOP: f64 = 160.0;
pub const TIMELINE_BOTTOM_RESERVE: f64 = 83.0;
/// Font size of the time-band captions drawn in the left gutter.
pub const BAND_CAPTION_PX: f64 = 9.5;

pub fn layout(s: &Sequence) -> Result<Scene, Vec<Diagnostic>> {
    let n = s.participants.len();
    let mut scene = Scene::default();
    let mut errors = Vec::new();

    let last_y = s
        .messages
        .iter()
        .map(|m| m.y)
        .chain(s.activations.iter().map(|a| a.to))
        .fold(0.0_f64, f64::max);
    let (view_w, view_h) = match s.meta.view_box {
        Some([w, h]) => (w, h),
        None => (
            (2.0 * SIDE_MARGIN + n as f64 * FIXED_BOX_W + (n as f64 - 1.0) * FIXED_GAP).ceil(),
            (last_y + TIMELINE_BOTTOM_RESERVE + 20.0).max(480.0).ceil(),
        ),
    };
    scene.view_w = view_w;
    scene.view_h = view_h;
    scene.timeline = Some((TIMELINE_TOP, view_h - TIMELINE_BOTTOM_RESERVE));

    // Time-band captions live in a gutter left of every lifeline so no
    // activation bar or arrow can overdraw them.
    let gutter = s
        .segments
        .iter()
        .map(|seg| geom::text_width(&seg.label, BAND_CAPTION_PX) + 16.0)
        .fold(0.0_f64, f64::max);
    let spread = s.meta.column_fit.as_deref() == Some("spread");
    let available = view_w - 2.0 * SIDE_MARGIN - gutter;
    let (box_w, gap) = if spread && n > 1 {
        let w = (available / n as f64 * 0.72).clamp(FIXED_BOX_W, 190.0);
        (w, (available - n as f64 * w) / (n as f64 - 1.0))
    } else {
        (FIXED_BOX_W, FIXED_GAP)
    };
    let total = n as f64 * box_w + (n as f64 - 1.0) * gap;
    let x0 = SIDE_MARGIN + gutter + ((available - total) / 2.0).max(0.0);

    let mut lifeline_x: HashMap<String, f64> = HashMap::new();
    let lifeline_bottom = view_h - 36.0;
    for (i, p) in s.participants.iter().enumerate() {
        let rect = Rect::new(x0 + i as f64 * (box_w + gap), BOX_Y, box_w, BOX_H);
        lifeline_x.insert(p.id.clone(), rect.cx());
        scene.lines.push(SceneLine {
            a: [rect.cx(), rect.bottom()],
            b: [rect.cx(), lifeline_bottom],
            dashed: true,
        });
        scene.nodes.push(SceneNode {
            id: p.id.clone(),
            rect,
            kind: p.kind,
            label: p.label.clone(),
            sublabel: p.sublabel.clone(),
            tag: None,
            label_px: NODE_LABEL_PX,
            sublabel_px: NODE_SUBLABEL_PX,
        });
    }

    for (i, seg) in s.segments.iter().enumerate() {
        if seg.to <= seg.from {
            errors.push(Diagnostic::error(
                "spec/segment-range",
                format!("segment \"{}\" must satisfy from < to", seg.label),
            ));
            continue;
        }
        scene.boxes.push(SceneBox {
            id: format!("segment-{i}"),
            kind: BoxKind::TimeBand,
            rect: Rect::new(
                SIDE_MARGIN - 8.0,
                seg.from,
                view_w - 2.0 * SIDE_MARGIN + 16.0,
                seg.to - seg.from,
            ),
            label: seg.label.clone(),
            variant: Variant::Default,
        });
    }

    for (i, m) in s.messages.iter().enumerate() {
        let (Some(&x1), Some(&x2)) = (lifeline_x.get(&m.from), lifeline_x.get(&m.to)) else {
            let missing = if lifeline_x.contains_key(&m.from) {
                &m.to
            } else {
                &m.from
            };
            errors.push(Diagnostic::error(
                "references/unknown-id",
                format!(
                    "message \"{}\" references unknown participant \"{missing}\"",
                    m.label
                ),
            ));
            continue;
        };
        let id = m.id.clone().unwrap_or_else(|| format!("message-{i}"));
        let points = if (x1 - x2).abs() < 1e-6 {
            vec![
                [x1, m.y],
                [x1 + 44.0, m.y],
                [x1 + 44.0, m.y + 20.0],
                [x1 + 6.0, m.y + 20.0],
            ]
        } else {
            vec![[x1, m.y], [x2, m.y]]
        };
        let label = Some(place_label(&points, &m.label, Some(0), None, 0.0, 0.0));
        scene.edges.push(SceneEdge {
            id,
            from: m.from.clone(),
            to: m.to.clone(),
            points,
            label,
            variant: m.variant,
            role: match m.variant {
                Variant::Return => EdgeRole::Return,
                Variant::Dashed => EdgeRole::Async,
                Variant::Emphasis => EdgeRole::Main,
                Variant::Security => EdgeRole::Error,
                Variant::Default => EdgeRole::Branch,
            },
            width: if m.variant == Variant::Emphasis {
                2.0
            } else {
                1.5
            },
        });
    }

    for a in &s.activations {
        let Some(&x) = lifeline_x.get(&a.participant) else {
            errors.push(Diagnostic::error(
                "references/unknown-id",
                format!(
                    "activation references unknown participant \"{}\"",
                    a.participant
                ),
            ));
            continue;
        };
        if a.to <= a.from {
            errors.push(Diagnostic::error(
                "spec/activation-range",
                format!("activation on \"{}\" must satisfy from < to", a.participant),
            ));
            continue;
        }
        let kind = a.kind.unwrap_or_else(|| {
            s.participants
                .iter()
                .find(|p| p.id == a.participant)
                .map(|p| p.kind)
                .unwrap()
        });
        scene.bars.push(SceneBar {
            owner: a.participant.clone(),
            rect: Rect::new(x - 5.0, a.from, 10.0, a.to - a.from),
            kind,
        });
    }

    if errors.is_empty() {
        Ok(scene)
    } else {
        Err(errors)
    }
}
