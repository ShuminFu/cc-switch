//! Typed session source identity (SESS-20).
//!
//! Historically every provider encoded its source as a free-form string:
//! a file path, a directory path, `sqlite:<db>:<ses_id>` (OpenCode) or
//! `sqlite:<db>#<id>` (Hermes). [`SessionSource`] is the single typed form;
//! the legacy string (`sourcePath`) is still emitted for backward
//! compatibility and parsed back here in exactly one place.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const SQLITE_PREFIX: &str = "sqlite:";

/// Where a session's transcript lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionSource {
    /// A single transcript file (JSONL / JSON / summary file).
    File { path: String },
    /// A directory holding the transcript (OpenCode legacy JSON storage).
    Dir { path: String },
    /// A row in another tool's SQLite database.
    Sqlite { db: String, id: String },
}

impl SessionSource {
    /// Parse the legacy `sourcePath` string for `provider_id`.
    pub fn from_legacy(provider_id: &str, raw: &str) -> Result<Self, String> {
        if let Some(rest) = raw.strip_prefix(SQLITE_PREFIX) {
            return parse_sqlite_ref(provider_id, rest)
                .ok_or_else(|| format!("Invalid SQLite source reference for {provider_id}"));
        }
        if raw.trim().is_empty() {
            return Err("Empty session source".to_string());
        }
        // OpenCode's legacy storage uses the per-session message directory.
        if provider_id == "opencode" {
            return Ok(Self::Dir {
                path: raw.to_string(),
            });
        }
        Ok(Self::File {
            path: raw.to_string(),
        })
    }

    /// Render the legacy `sourcePath` string understood by older frontends.
    pub fn to_legacy(&self, provider_id: &str) -> String {
        match self {
            Self::File { path } | Self::Dir { path } => path.clone(),
            Self::Sqlite { db, id } => {
                if provider_id == "opencode" {
                    format!("{SQLITE_PREFIX}{db}:{id}")
                } else {
                    format!("{SQLITE_PREFIX}{db}#{id}")
                }
            }
        }
    }

    /// Filesystem path backing this source (the database for SQLite sources).
    pub fn fs_path(&self) -> PathBuf {
        match self {
            Self::File { path } | Self::Dir { path } => PathBuf::from(path),
            Self::Sqlite { db, .. } => PathBuf::from(db),
        }
    }
}

/// OpenCode: `<db>:<ses_id>` (split on the last `:ses_` because Windows paths
/// contain colons). Hermes and others: `<db>#<id>`.
fn parse_sqlite_ref(provider_id: &str, rest: &str) -> Option<SessionSource> {
    let (db, id) = if provider_id == "opencode" {
        let sep = rest.rfind(":ses_")?;
        (&rest[..sep], &rest[sep + 1..])
    } else {
        let sep = rest.rfind('#')?;
        (&rest[..sep], &rest[sep + 1..])
    };
    if db.is_empty() || id.is_empty() {
        return None;
    }
    Some(SessionSource::Sqlite {
        db: db.to_string(),
        id: id.to_string(),
    })
}

/// Stable identifier for a session, computed server-side so the frontend
/// never has to build keys from source strings.
pub fn session_key(provider_id: &str, session_id: &str, legacy_source: &str) -> String {
    format!("{provider_id}:{session_id}:{legacy_source}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_opencode_sqlite_with_windows_path() {
        let source =
            SessionSource::from_legacy("opencode", r"sqlite:C:\Users\me\opencode.db:ses_abc")
                .unwrap();
        assert_eq!(
            source,
            SessionSource::Sqlite {
                db: r"C:\Users\me\opencode.db".to_string(),
                id: "ses_abc".to_string()
            }
        );
        assert_eq!(
            source.to_legacy("opencode"),
            r"sqlite:C:\Users\me\opencode.db:ses_abc"
        );
    }

    #[test]
    fn parses_hermes_sqlite_and_round_trips() {
        let raw = "sqlite:/home/u/.hermes/state.db#session-1";
        let source = SessionSource::from_legacy("hermes", raw).unwrap();
        assert_eq!(
            source,
            SessionSource::Sqlite {
                db: "/home/u/.hermes/state.db".to_string(),
                id: "session-1".to_string()
            }
        );
        assert_eq!(source.to_legacy("hermes"), raw);
    }

    #[test]
    fn rejects_malformed_sqlite_refs() {
        assert!(SessionSource::from_legacy("hermes", "sqlite:").is_err());
        assert!(SessionSource::from_legacy("hermes", "sqlite:/db#").is_err());
        assert!(SessionSource::from_legacy("opencode", "sqlite:/db#x").is_err());
        assert!(SessionSource::from_legacy("claude", "").is_err());
    }

    #[test]
    fn file_and_dir_sources() {
        assert_eq!(
            SessionSource::from_legacy("claude", "/a/b.jsonl").unwrap(),
            SessionSource::File {
                path: "/a/b.jsonl".to_string()
            }
        );
        assert_eq!(
            SessionSource::from_legacy("opencode", "/s/message/ses_1").unwrap(),
            SessionSource::Dir {
                path: "/s/message/ses_1".to_string()
            }
        );
    }

    #[test]
    fn serializes_as_tagged_union() {
        let json = serde_json::to_value(SessionSource::Sqlite {
            db: "/d".into(),
            id: "x".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "sqlite", "db": "/d", "id": "x"})
        );
    }
}
