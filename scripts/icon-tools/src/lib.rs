//! Maintenance tooling for the provider icon assets that live in
//! `src/icons/extracted`.
//!
//! This crate replaces the former `scripts/extract-icons.js` and
//! `scripts/filter-icons.js` Node scripts. It is dependency free so that it
//! builds quickly with the repository toolchain and needs no network access.
//!
//! Four operations are exposed through the `icon-tools` binary:
//!
//! * [`extract`] copies icons from the `@lobehub/icons-static-svg` package.
//! * [`filter`] removes SVGs that are not on the keep list and prefers colour
//!   variants, without touching files that the application references.
//! * [`check`] validates that `index.ts`, `metadata.ts` and the files on disk
//!   agree with each other.
//! * [`index`] renders a fresh `index.ts` from the files in a directory.

pub mod check;
pub mod extract;
pub mod filter;
pub mod index;
pub mod svg;
pub mod util;
