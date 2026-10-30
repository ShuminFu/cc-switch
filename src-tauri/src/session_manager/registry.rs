//! Session provider registry (SESS-09).
//!
//! Every supported tool implements [`SessionProvider`]; scanning, reading and
//! deleting dispatch through [`PROVIDERS`] instead of hand-written `match`
//! arms. Adding a provider means implementing the trait and listing it here.

use std::path::{Path, PathBuf};

use super::cache::ScanCache;
use super::providers::{claude, codex, gemini, grokbuild, hermes, openclaw, opencode};
use super::source::SessionSource;
use super::{SessionMessage, SessionMeta};

pub trait SessionProvider: Send + Sync {
    /// Stable provider id (`"claude"`, `"codex"`, ...).
    fn id(&self) -> &'static str;
    /// Directories that may contain this provider's file-based sessions.
    fn roots(&self) -> Vec<PathBuf>;
    /// The SQLite database this provider reads, if any.
    fn sqlite_db(&self) -> Option<PathBuf> {
        None
    }
    /// List sessions, using `cache` to skip unchanged files.
    fn scan(&self, cache: &ScanCache) -> Vec<SessionMeta>;
    /// Load the transcript of an already validated source.
    fn load_messages(&self, source: &ValidatedSource) -> Result<Vec<SessionMessage>, String>;
    /// Delete an already validated source.
    fn delete(&self, source: &ValidatedSource, session_id: &str) -> Result<bool, String>;
}

/// A source that was checked against the provider's roots / database.
#[derive(Debug, Clone)]
pub struct ValidatedSource {
    pub source: SessionSource,
    /// Canonical path of the file / directory / database.
    pub path: PathBuf,
    /// Canonical provider root containing `path` (file and dir sources).
    pub root: Option<PathBuf>,
}

impl ValidatedSource {
    fn legacy(&self, provider_id: &str) -> String {
        match &self.source {
            SessionSource::Sqlite { id, .. } => SessionSource::Sqlite {
                db: self.path.to_string_lossy().to_string(),
                id: id.clone(),
            }
            .to_legacy(provider_id),
            _ => self.path.to_string_lossy().to_string(),
        }
    }

    fn root_or_parent(&self) -> &Path {
        self.root
            .as_deref()
            .or_else(|| self.path.parent())
            .unwrap_or(&self.path)
    }
}

pub const OUTSIDE_ROOTS: &str = "Session source is outside provider roots";

/// Check that `source` lives under one of `roots` (file / dir sources) or is
/// exactly `expected_db` (SQLite sources). Paths are canonicalized, so `..`
/// segments and symlinks cannot escape (SESS-13).
pub fn validate_source_with(
    source: &SessionSource,
    roots: &[PathBuf],
    expected_db: Option<&Path>,
) -> Result<ValidatedSource, String> {
    match source {
        SessionSource::Sqlite { db, .. } => {
            let expected = expected_db.ok_or_else(|| OUTSIDE_ROOTS.to_string())?;
            let canonical = Path::new(db)
                .canonicalize()
                .map_err(|_| OUTSIDE_ROOTS.to_string())?;
            let expected = expected
                .canonicalize()
                .map_err(|_| OUTSIDE_ROOTS.to_string())?;
            if canonical != expected {
                return Err(OUTSIDE_ROOTS.to_string());
            }
            Ok(ValidatedSource {
                source: source.clone(),
                path: canonical,
                root: None,
            })
        }
        SessionSource::File { path } | SessionSource::Dir { path } => {
            let path = Path::new(path);
            if !path.exists() {
                return Err(format!("session source not found: {}", path.display()));
            }
            let canonical = path
                .canonicalize()
                .map_err(|e| format!("Failed to resolve session source: {e}"))?;
            for root in roots {
                let Ok(root) = root.canonicalize() else {
                    continue;
                };
                if canonical.starts_with(&root) && canonical != root {
                    return Ok(ValidatedSource {
                        source: source.clone(),
                        path: canonical,
                        root: Some(root),
                    });
                }
            }
            Err(OUTSIDE_ROOTS.to_string())
        }
    }
}

pub fn validate_source(
    provider: &dyn SessionProvider,
    source: &SessionSource,
) -> Result<ValidatedSource, String> {
    validate_source_with(source, &provider.roots(), provider.sqlite_db().as_deref())
}

pub fn find_provider(provider_id: &str) -> Result<&'static dyn SessionProvider, String> {
    PROVIDERS
        .iter()
        .copied()
        .find(|p| p.id() == provider_id)
        .ok_or_else(|| format!("Unsupported provider: {provider_id}"))
}

macro_rules! file_provider {
    ($name:ident, $id:literal, $module:ident, roots = $roots:expr) => {
        pub struct $name;
        impl SessionProvider for $name {
            fn id(&self) -> &'static str {
                $id
            }
            fn roots(&self) -> Vec<PathBuf> {
                $roots
            }
            fn scan(&self, cache: &ScanCache) -> Vec<SessionMeta> {
                $module::scan_sessions(cache)
            }
            fn load_messages(
                &self,
                source: &ValidatedSource,
            ) -> Result<Vec<SessionMessage>, String> {
                $module::load_messages(&source.path)
            }
            fn delete(&self, source: &ValidatedSource, session_id: &str) -> Result<bool, String> {
                $module::delete_session(source.root_or_parent(), &source.path, session_id)
            }
        }
    };
}

file_provider!(
    ClaudeProvider,
    "claude",
    claude,
    roots = vec![crate::config::get_claude_config_dir().join("projects")]
);
file_provider!(
    CodexProvider,
    "codex",
    codex,
    roots = codex::session_roots()
);
file_provider!(
    GeminiProvider,
    "gemini",
    gemini,
    roots = vec![crate::gemini_config::get_gemini_dir().join("tmp")]
);
file_provider!(
    GrokBuildProvider,
    "grokbuild",
    grokbuild,
    roots = grokbuild::session_roots()
);
file_provider!(
    OpenClawProvider,
    "openclaw",
    openclaw,
    roots = vec![crate::openclaw_config::get_openclaw_dir().join("agents")]
);

/// OpenCode: SQLite database plus the legacy JSON storage directory.
pub struct OpenCodeProvider;
impl SessionProvider for OpenCodeProvider {
    fn id(&self) -> &'static str {
        "opencode"
    }
    fn roots(&self) -> Vec<PathBuf> {
        vec![opencode::get_opencode_data_dir()]
    }
    fn sqlite_db(&self) -> Option<PathBuf> {
        Some(opencode::get_opencode_base_dir().join("opencode.db"))
    }
    fn scan(&self, cache: &ScanCache) -> Vec<SessionMeta> {
        opencode::scan_sessions(cache)
    }
    fn load_messages(&self, source: &ValidatedSource) -> Result<Vec<SessionMessage>, String> {
        match source.source {
            SessionSource::Sqlite { .. } => {
                opencode::load_messages_sqlite(&source.legacy(self.id()))
            }
            _ => opencode::load_messages(&source.path),
        }
    }
    fn delete(&self, source: &ValidatedSource, session_id: &str) -> Result<bool, String> {
        match source.source {
            SessionSource::Sqlite { .. } => {
                opencode::delete_session_sqlite(session_id, &source.legacy(self.id()))
            }
            _ => opencode::delete_session(source.root_or_parent(), &source.path, session_id),
        }
    }
}

/// Hermes: `state.db` plus JSONL transcripts under `sessions/`.
pub struct HermesProvider;
impl SessionProvider for HermesProvider {
    fn id(&self) -> &'static str {
        "hermes"
    }
    fn roots(&self) -> Vec<PathBuf> {
        vec![crate::hermes_config::get_hermes_dir().join("sessions")]
    }
    fn sqlite_db(&self) -> Option<PathBuf> {
        Some(crate::hermes_config::get_hermes_dir().join("state.db"))
    }
    fn scan(&self, cache: &ScanCache) -> Vec<SessionMeta> {
        hermes::scan_sessions(cache)
    }
    fn load_messages(&self, source: &ValidatedSource) -> Result<Vec<SessionMessage>, String> {
        match source.source {
            SessionSource::Sqlite { .. } => hermes::load_messages_sqlite(&source.legacy(self.id())),
            _ => hermes::load_messages(&source.path),
        }
    }
    fn delete(&self, source: &ValidatedSource, session_id: &str) -> Result<bool, String> {
        match source.source {
            SessionSource::Sqlite { .. } => {
                hermes::delete_session_sqlite(session_id, &source.legacy(self.id()))
            }
            _ => hermes::delete_session(source.root_or_parent(), &source.path, session_id),
        }
    }
}

pub static PROVIDERS: &[&dyn SessionProvider] = &[
    &CodexProvider,
    &ClaudeProvider,
    &OpenCodeProvider,
    &OpenClawProvider,
    &GeminiProvider,
    &HermesProvider,
    &GrokBuildProvider,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source_must_be_inside_a_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let inside_file = root.path().join("a.jsonl");
        let outside_file = outside.path().join("b.jsonl");
        std::fs::write(&inside_file, "{}").unwrap();
        std::fs::write(&outside_file, "{}").unwrap();
        let roots = vec![root.path().to_path_buf()];

        let ok = validate_source_with(
            &SessionSource::File {
                path: inside_file.to_string_lossy().to_string(),
            },
            &roots,
            None,
        )
        .unwrap();
        assert!(ok.root.is_some());

        let err = validate_source_with(
            &SessionSource::File {
                path: outside_file.to_string_lossy().to_string(),
            },
            &roots,
            None,
        )
        .unwrap_err();
        assert!(err.contains("outside provider roots"));

        // `..` traversal is resolved before the check.
        let sneaky = root.path().join("..").join(
            outside_file
                .strip_prefix(outside.path().parent().unwrap())
                .unwrap(),
        );
        let err = validate_source_with(
            &SessionSource::File {
                path: sneaky.to_string_lossy().to_string(),
            },
            &roots,
            None,
        )
        .unwrap_err();
        assert!(err.contains("outside provider roots"));
    }

    #[test]
    fn sqlite_source_must_match_expected_db() {
        let dir = tempfile::tempdir().unwrap();
        let expected = dir.path().join("state.db");
        let other = dir.path().join("other.db");
        std::fs::write(&expected, "").unwrap();
        std::fs::write(&other, "").unwrap();

        assert!(validate_source_with(
            &SessionSource::Sqlite {
                db: expected.to_string_lossy().to_string(),
                id: "x".into()
            },
            &[],
            Some(&expected)
        )
        .is_ok());
        let err = validate_source_with(
            &SessionSource::Sqlite {
                db: other.to_string_lossy().to_string(),
                id: "x".into(),
            },
            &[],
            Some(&expected),
        )
        .unwrap_err();
        assert!(err.contains("outside provider roots"));
        // Providers without a database reject SQLite sources entirely.
        assert!(validate_source_with(
            &SessionSource::Sqlite {
                db: expected.to_string_lossy().to_string(),
                id: "x".into()
            },
            &[],
            None
        )
        .is_err());
    }

    #[test]
    fn every_provider_id_is_unique() {
        let mut ids: Vec<_> = PROVIDERS.iter().map(|p| p.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PROVIDERS.len());
    }
}
