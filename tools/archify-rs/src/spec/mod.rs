//! Typed specification model. The JSON shapes mirror the archify schemas for
//! the `architecture`, `sequence` and `workflow` diagram types; unknown fields
//! are rejected so authoring mistakes surface as errors instead of silently
//! rendering nothing.

use serde::Deserialize;
use std::fmt;

pub type Point = [f64; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Frontend,
    Backend,
    Database,
    Cloud,
    Security,
    Messagebus,
    External,
}

impl ComponentType {
    pub const ALL: [ComponentType; 7] = [
        ComponentType::Frontend,
        ComponentType::Backend,
        ComponentType::Database,
        ComponentType::Cloud,
        ComponentType::Security,
        ComponentType::Messagebus,
        ComponentType::External,
    ];

    pub fn css(self) -> &'static str {
        match self {
            ComponentType::Frontend => "frontend",
            ComponentType::Backend => "backend",
            ComponentType::Database => "database",
            ComponentType::Cloud => "cloud",
            ComponentType::Security => "security",
            ComponentType::Messagebus => "messagebus",
            ComponentType::External => "external",
        }
    }

    pub fn legend_label(self) -> &'static str {
        match self {
            ComponentType::Frontend => "Frontend",
            ComponentType::Backend => "Backend",
            ComponentType::Database => "Database",
            ComponentType::Cloud => "Cloud",
            ComponentType::Security => "Security",
            ComponentType::Messagebus => "Message bus",
            ComponentType::External => "External",
        }
    }
}

impl fmt::Display for ComponentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.css())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    #[default]
    Default,
    Emphasis,
    Security,
    Dashed,
    /// Only meaningful for sequence messages: a quiet reply arrow.
    Return,
}

impl Variant {
    pub fn css(self) -> &'static str {
        match self {
            Variant::Default => "default",
            Variant::Emphasis => "emphasis",
            Variant::Security => "security",
            Variant::Dashed => "dashed",
            Variant::Return => "return",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub dot: String,
    pub title: String,
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    pub label: String,
    pub focus: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub title: String,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub animation: Option<String>,
    #[serde(default)]
    pub visual_preset: Option<String>,
    #[serde(default)]
    pub quality_profile: Option<String>,
    #[serde(default)]
    pub engineering_profile: Option<String>,
    #[serde(default)]
    pub repository: Option<serde_json::Value>,
    #[serde(default)]
    pub legend: Option<serde_json::Value>,
    #[serde(default, rename = "viewBox")]
    pub view_box: Option<[f64; 2]>,
    #[serde(default)]
    pub views: Vec<View>,
    #[serde(default)]
    pub column_fit: Option<String>,
}

// ---------------------------------------------------------------- architecture

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridLayout {
    pub mode: String,
    #[serde(default)]
    pub origin: Option<Point>,
    #[serde(default)]
    pub cols: Option<u32>,
    #[serde(default, rename = "gapX")]
    pub gap_x: Option<f64>,
    #[serde(default, rename = "gapY")]
    pub gap_y: Option<f64>,
    #[serde(default, rename = "cellW")]
    pub cell_w: Option<f64>,
    #[serde(default, rename = "cellH")]
    pub cell_h: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ComponentType,
    pub label: String,
    #[serde(default)]
    pub sublabel: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub brand: Option<serde_json::Value>,
    #[serde(default)]
    pub sources: Option<serde_json::Value>,
    #[serde(default)]
    pub row: Option<u32>,
    #[serde(default)]
    pub col: Option<u32>,
    #[serde(default)]
    pub pos: Option<Point>,
    #[serde(default)]
    pub size: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub kind: String,
    pub label: String,
    pub wraps: Vec<String>,
    #[serde(default)]
    pub pad: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    #[serde(default)]
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub variant: Variant,
    #[serde(default, rename = "fromSide")]
    pub from_side: Option<Side>,
    #[serde(default, rename = "toSide")]
    pub to_side: Option<Side>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub via: Vec<Point>,
    #[serde(default, rename = "labelAt")]
    pub label_at: Option<Point>,
    #[serde(default, rename = "labelDx")]
    pub label_dx: Option<f64>,
    #[serde(default, rename = "labelDy")]
    pub label_dy: Option<f64>,
    #[serde(default, rename = "labelSegment")]
    pub label_segment: Option<usize>,
    #[serde(default)]
    pub width: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Architecture {
    pub schema_version: u32,
    pub diagram_type: String,
    pub meta: Meta,
    #[serde(default)]
    pub layout: Option<GridLayout>,
    pub components: Vec<Component>,
    #[serde(default)]
    pub boundaries: Vec<Boundary>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub cards: Vec<Card>,
}

// -------------------------------------------------------------------- sequence

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ComponentType,
    pub label: String,
    #[serde(default)]
    pub sublabel: Option<String>,
    #[serde(default)]
    pub brand: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeSegment {
    pub from: f64,
    pub to: f64,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    #[serde(default)]
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    pub y: f64,
    pub label: String,
    #[serde(default)]
    pub variant: Variant,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activation {
    pub participant: String,
    pub from: f64,
    pub to: f64,
    #[serde(default, rename = "type")]
    pub kind: Option<ComponentType>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub schema_version: u32,
    pub diagram_type: String,
    pub meta: Meta,
    pub participants: Vec<Participant>,
    #[serde(default)]
    pub segments: Vec<TimeSegment>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub activations: Vec<Activation>,
    #[serde(default)]
    pub cards: Vec<Card>,
}

// -------------------------------------------------------------------- workflow

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub variant: Option<String>,
}

impl Lane {
    pub fn is_exception(&self) -> bool {
        self.variant.as_deref() == Some("exception")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    pub id: String,
    pub label: String,
    #[serde(rename = "fromCol")]
    pub from_col: u32,
    #[serde(rename = "toCol")]
    pub to_col: u32,
    #[serde(default)]
    pub variant: Variant,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub lane: String,
    #[serde(rename = "fromCol")]
    pub from_col: u32,
    #[serde(rename = "toCol")]
    pub to_col: u32,
    #[serde(default)]
    pub variant: Variant,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SemanticChecks {
    #[serde(default, rename = "allowedRoots")]
    pub allowed_roots: Option<Vec<String>>,
    #[serde(default, rename = "allowedTerminals")]
    pub allowed_terminals: Option<Vec<String>>,
    #[serde(default, rename = "requiredEdges")]
    pub required_edges: Option<Vec<Relation>>,
    #[serde(default, rename = "requiredPaths")]
    pub required_paths: Option<Vec<Relation>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WfNode {
    pub id: String,
    pub lane: String,
    pub col: u32,
    #[serde(rename = "type")]
    pub kind: ComponentType,
    pub label: String,
    #[serde(default)]
    pub sublabel: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub brand: Option<serde_json::Value>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default, rename = "yOffset")]
    pub y_offset: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WfEdge {
    #[serde(default)]
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub variant: Variant,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default, rename = "fromSide")]
    pub from_side: Option<Side>,
    #[serde(default, rename = "toSide")]
    pub to_side: Option<Side>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub via: Vec<Point>,
    #[serde(default, rename = "labelAt")]
    pub label_at: Option<Point>,
    #[serde(default, rename = "labelDx")]
    pub label_dx: Option<f64>,
    #[serde(default, rename = "labelDy")]
    pub label_dy: Option<f64>,
    #[serde(default, rename = "labelSegment")]
    pub label_segment: Option<usize>,
    #[serde(default, rename = "channelX")]
    pub channel_x: Option<f64>,
    #[serde(default, rename = "channelY")]
    pub channel_y: Option<f64>,
    #[serde(default)]
    pub bias: Option<f64>,
    #[serde(default)]
    pub width: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub schema_version: u32,
    pub diagram_type: String,
    pub meta: Meta,
    pub lanes: Vec<Lane>,
    #[serde(default)]
    pub phases: Vec<Phase>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default, rename = "mainPath")]
    pub main_path: Option<Vec<String>>,
    #[serde(default, rename = "semanticChecks")]
    pub semantic_checks: Option<SemanticChecks>,
    pub nodes: Vec<WfNode>,
    #[serde(default)]
    pub edges: Vec<WfEdge>,
    #[serde(default)]
    pub cards: Vec<Card>,
}

// ----------------------------------------------------------------- dispatcher

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramType {
    Architecture,
    Sequence,
    Workflow,
}

impl DiagramType {
    pub fn parse(s: &str) -> Option<DiagramType> {
        match s {
            "architecture" => Some(DiagramType::Architecture),
            "sequence" => Some(DiagramType::Sequence),
            "workflow" => Some(DiagramType::Workflow),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DiagramType::Architecture => "architecture",
            DiagramType::Sequence => "sequence",
            DiagramType::Workflow => "workflow",
        }
    }
}

impl fmt::Display for DiagramType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone)]
pub enum Spec {
    Architecture(Architecture),
    Sequence(Sequence),
    Workflow(Workflow),
}

impl Spec {
    pub fn diagram_type(&self) -> DiagramType {
        match self {
            Spec::Architecture(_) => DiagramType::Architecture,
            Spec::Sequence(_) => DiagramType::Sequence,
            Spec::Workflow(_) => DiagramType::Workflow,
        }
    }

    pub fn meta(&self) -> &Meta {
        match self {
            Spec::Architecture(a) => &a.meta,
            Spec::Sequence(s) => &s.meta,
            Spec::Workflow(w) => &w.meta,
        }
    }

    pub fn cards(&self) -> &[Card] {
        match self {
            Spec::Architecture(a) => &a.cards,
            Spec::Sequence(s) => &s.cards,
            Spec::Workflow(w) => &w.cards,
        }
    }

    /// Parse JSON bytes as the requested diagram type. The document's own
    /// `diagram_type` must agree with the requested type.
    pub fn from_json(kind: DiagramType, bytes: &[u8]) -> Result<Spec, String> {
        let probe: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
        let declared = probe
            .get("diagram_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing /diagram_type".to_string())?;
        if declared != kind.name() {
            return Err(format!(
                "/diagram_type is \"{declared}\" but the command asked for \"{kind}\""
            ));
        }
        let spec = match kind {
            DiagramType::Architecture => serde_json::from_value::<Architecture>(probe)
                .map(Spec::Architecture)
                .map_err(|e| format!("architecture spec: {e}"))?,
            DiagramType::Sequence => serde_json::from_value::<Sequence>(probe)
                .map(Spec::Sequence)
                .map_err(|e| format!("sequence spec: {e}"))?,
            DiagramType::Workflow => serde_json::from_value::<Workflow>(probe)
                .map(Spec::Workflow)
                .map_err(|e| format!("workflow spec: {e}"))?,
        };
        spec.check_structure()?;
        Ok(spec)
    }

    /// Cheap structural rules that do not need geometry: schema version,
    /// non-empty collections, identifier syntax, uniqueness.
    fn check_structure(&self) -> Result<(), String> {
        fn ident_ok(id: &str) -> bool {
            let mut chars = id.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }
        fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Result<(), String> {
            let mut seen = std::collections::HashSet::new();
            for id in ids {
                if !ident_ok(id) {
                    return Err(format!(
                        "{what} id \"{id}\" must match ^[a-zA-Z][a-zA-Z0-9_-]*$"
                    ));
                }
                if !seen.insert(id) {
                    return Err(format!("duplicate {what} id \"{id}\""));
                }
            }
            Ok(())
        }
        if self.meta().title.trim().is_empty() {
            return Err("/meta/title must not be empty".into());
        }
        match self {
            Spec::Architecture(a) => {
                if a.schema_version != 1 {
                    return Err("architecture requires schema_version 1".into());
                }
                if a.components.is_empty() {
                    return Err("/components must contain at least one component".into());
                }
                unique("component", a.components.iter().map(|c| c.id.as_str()))?;
                unique(
                    "connection",
                    a.connections.iter().filter_map(|c| c.id.as_deref()),
                )?;
            }
            Spec::Sequence(s) => {
                if s.schema_version != 1 {
                    return Err("sequence requires schema_version 1".into());
                }
                if s.participants.len() < 2 {
                    return Err("/participants must contain at least two participants".into());
                }
                if s.messages.is_empty() {
                    return Err("/messages must contain at least one message".into());
                }
                unique("participant", s.participants.iter().map(|p| p.id.as_str()))?;
                unique("message", s.messages.iter().filter_map(|m| m.id.as_deref()))?;
                if let Some(vb) = s.meta.view_box {
                    if vb[0] < 480.0 || vb[1] < 480.0 {
                        return Err(format!(
                            "/meta/viewBox must be at least 480x480 (got {}x{})",
                            vb[0], vb[1]
                        ));
                    }
                }
            }
            Spec::Workflow(w) => {
                if !(w.schema_version == 1 || w.schema_version == 2) {
                    return Err("workflow requires schema_version 1 or 2".into());
                }
                if w.lanes.is_empty() {
                    return Err("/lanes must contain at least one lane".into());
                }
                if w.nodes.is_empty() {
                    return Err("/nodes must contain at least one node".into());
                }
                unique("lane", w.lanes.iter().map(|l| l.id.as_str()))?;
                unique("node", w.nodes.iter().map(|n| n.id.as_str()))?;
                unique("edge", w.edges.iter().filter_map(|e| e.id.as_deref()))?;
                unique("phase", w.phases.iter().map(|p| p.id.as_str()))?;
                unique("group", w.groups.iter().map(|g| g.id.as_str()))?;
                for n in &w.nodes {
                    if n.col > 5 {
                        return Err(format!("node \"{}\" col must be 0..5", n.id));
                    }
                }
                for p in &w.phases {
                    if p.from_col > p.to_col || p.to_col > 5 {
                        return Err(format!(
                            "phase \"{}\" columns must satisfy 0 <= fromCol <= toCol <= 5",
                            p.id
                        ));
                    }
                }
                if let Some(vb) = w.meta.view_box {
                    if vb[0] < 700.0 || vb[1] < 240.0 {
                        return Err("/meta/viewBox must be at least 700x240".into());
                    }
                }
            }
        }
        for v in &self.meta().views {
            if v.focus.is_empty() {
                return Err(format!("view \"{}\" must focus at least one id", v.id));
            }
        }
        Ok(())
    }
}
