//! Maintenance tooling for the provider icons bundled with CC Switch.
//!
//! The icons live in `src/icons/extracted`. Two hand-maintained TypeScript
//! files sit next to them:
//!
//! * `index.ts` holds every inline SVG (the `icons` map) and every raster or
//!   oversized icon that is imported by URL (the `iconUrls` map).
//! * `metadata.ts` holds display names, categories, keywords and colours.
//!
//! This crate replaces the former `scripts/extract-icons.js` and
//! `scripts/filter-icons.js`. Unlike those scripts it never rewrites the two
//! TypeScript files wholesale: it only appends entries that are missing, so
//! hand-tuned icons and metadata survive every run.

pub mod check;
pub mod extract;
pub mod filter;
pub mod svg;
pub mod ts;

use std::fmt;
use std::path::{Path, PathBuf};

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for the tool. Every failure is reported as a message that the
/// CLI prints verbatim.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error(err.to_string())
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error(msg)
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Error(msg.to_string())
    }
}

/// Wraps an I/O error with the path that was being accessed.
pub fn io_context<T>(res: std::io::Result<T>, what: &str, path: &Path) -> Result<T> {
    res.map_err(|e| Error(format!("{what} {}: {e}", path.display())))
}

/// Locations the tool works with.
#[derive(Debug, Clone)]
pub struct Config {
    /// Repository root.
    pub root: PathBuf,
    /// Directory holding the bundled icons and the two TypeScript files.
    pub icons_dir: PathBuf,
    /// Directory holding the upstream `@lobehub/icons-static-svg` SVG files.
    pub source_dir: PathBuf,
}

impl Config {
    /// Builds a configuration rooted at `root` with the default layout.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Config {
            icons_dir: root.join("src").join("icons").join("extracted"),
            source_dir: root
                .join("node_modules")
                .join("@lobehub")
                .join("icons-static-svg")
                .join("icons"),
            root,
        }
    }

    /// Repository root derived from this crate's location (`scripts/icons`).
    pub fn default_root() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or(manifest_dir)
    }

    /// Path of `index.ts`.
    pub fn index_path(&self) -> PathBuf {
        self.icons_dir.join("index.ts")
    }

    /// Path of `metadata.ts`.
    pub fn metadata_path(&self) -> PathBuf {
        self.icons_dir.join("metadata.ts")
    }
}

/// Lists the file names of every regular file in `dir`, sorted.
pub fn list_files(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in io_context(std::fs::read_dir(dir), "cannot read", dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Reads a UTF-8 text file.
pub fn read_text(path: &Path) -> Result<String> {
    io_context(std::fs::read_to_string(path), "cannot read", path)
}

/// Writes a text file, creating parent directories as needed.
pub fn write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        io_context(std::fs::create_dir_all(parent), "cannot create", parent)?;
    }
    io_context(std::fs::write(path, content), "cannot write", path)
}
