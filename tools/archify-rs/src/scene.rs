//! Resolved geometry shared by the validator and the renderer. Each diagram
//! type lays itself out into this neutral model so the checks and the SVG
//! emitter never need to know which type they are looking at.

use crate::geom::Rect;
use crate::spec::{ComponentType, Point, Variant};

#[derive(Debug, Clone)]
pub struct SceneNode {
    pub id: String,
    pub rect: Rect,
    pub kind: ComponentType,
    pub label: String,
    pub sublabel: Option<String>,
    pub tag: Option<String>,
    /// Font sizes used for the readability projection.
    pub label_px: f64,
    pub sublabel_px: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    Region,
    SecurityGroup,
    Lane,
    ExceptionLane,
    Group,
    Phase,
    TimeBand,
}

#[derive(Debug, Clone)]
pub struct SceneBox {
    pub id: String,
    pub kind: BoxKind,
    pub rect: Rect,
    pub label: String,
    pub variant: Variant,
}

#[derive(Debug, Clone)]
pub struct SceneLabel {
    pub text: String,
    pub rect: Rect,
    /// Text anchor point (center) and whether it sits on a horizontal run.
    pub anchor: Point,
    pub horizontal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeRole {
    Main,
    Branch,
    Async,
    Return,
    Error,
}

#[derive(Debug, Clone)]
pub struct SceneEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub points: Vec<Point>,
    pub label: Option<SceneLabel>,
    pub variant: Variant,
    pub role: EdgeRole,
    pub width: f64,
}

/// A decorative line with no arrowhead (sequence lifelines).
#[derive(Debug, Clone)]
pub struct SceneLine {
    pub a: Point,
    pub b: Point,
    pub dashed: bool,
}

/// A filled bar (sequence activations).
#[derive(Debug, Clone)]
pub struct SceneBar {
    pub owner: String,
    pub rect: Rect,
    pub kind: ComponentType,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub view_w: f64,
    pub view_h: f64,
    pub nodes: Vec<SceneNode>,
    pub boxes: Vec<SceneBox>,
    pub edges: Vec<SceneEdge>,
    pub lines: Vec<SceneLine>,
    pub bars: Vec<SceneBar>,
    /// Sequence-only: the readable timeline range for message rows.
    pub timeline: Option<(f64, f64)>,
}

impl Scene {
    pub fn node(&self, id: &str) -> Option<&SceneNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Bounding box of every drawn element, used to size an automatic viewBox.
    pub fn content_bounds(&self) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        let mut push = |r: Rect| {
            acc = Some(match acc {
                Some(a) => a.union(&r),
                None => r,
            });
        };
        for n in &self.nodes {
            push(n.rect);
        }
        for b in &self.boxes {
            push(b.rect);
        }
        for e in &self.edges {
            for p in &e.points {
                push(Rect::new(p[0], p[1], 0.0, 0.0));
            }
            if let Some(l) = &e.label {
                push(l.rect);
            }
        }
        for l in &self.lines {
            push(Rect::new(
                l.a[0].min(l.b[0]),
                l.a[1].min(l.b[1]),
                (l.a[0] - l.b[0]).abs(),
                (l.a[1] - l.b[1]).abs(),
            ));
        }
        for b in &self.bars {
            push(b.rect);
        }
        acc
    }

    pub fn kinds_present(&self) -> Vec<(ComponentType, usize)> {
        ComponentType::ALL
            .iter()
            .map(|k| (*k, self.nodes.iter().filter(|n| n.kind == *k).count()))
            .filter(|(_, c)| *c > 0)
            .collect()
    }
}

pub const LABEL_FONT_PX: f64 = 11.0;
pub const NODE_LABEL_PX: f64 = 12.5;
pub const NODE_SUBLABEL_PX: f64 = 9.0;
pub const LABEL_H: f64 = 14.0;
