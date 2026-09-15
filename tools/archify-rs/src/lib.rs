//! archify-rs: a Rust compiler for typed-JSON diagram specifications.
//!
//! The pipeline is `spec` (parse) → `layout` (resolve geometry into a
//! [`scene::Scene`]) → `validate` (geometry checks) → `render` (standalone
//! HTML with inline SVG) → `receipt` (deterministic delivery evidence).

pub mod diag;
pub mod geom;
pub mod layout;
pub mod receipt;
pub mod render;
pub mod scene;
pub mod spec;
pub mod validate;
