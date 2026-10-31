//! Startup data-migration registry (CFG-10, first increment).
//!
//! One-shot migrations of user data outside the SQLite schema (Codex
//! session history buckets, provider templates) are registered here instead
//! of being called one by one from `lib.rs`. Each declares the version it
//! was introduced in and when it may be retired, and whether it rewrites
//! Codex session files; those are deferred while Codex looks active, because
//! rewriting a JSONL file Codex is appending to loses data.
//!
//! The per-migration completion ledger is still the settings flags each
//! migration checks itself; outcomes are returned for logging and, later,
//! a Settings "migrations" panel.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::database::Database;
use crate::error::AppError;

/// What a migration run did, for logs and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// The migration changed data; the text summarises what.
    Applied(String),
    /// Nothing to do (already applied, disabled, no data).
    Skipped(String),
}

/// What the runner decided for one migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationReport {
    Ran(MigrationOutcome),
    /// Not run this time; it is retried on a later startup.
    Deferred(&'static str),
    Failed(String),
}

/// State the runner and the migrations share.
pub struct MigrationContext<'a> {
    pub db: &'a Database,
    /// Codex looks like it is running (see [`codex_appears_active`]).
    pub codex_active: bool,
}

pub trait Migration: Sync {
    fn id(&self) -> &'static str;
    /// cc-switch version that introduced the migration (or registered it
    /// here, for migrations older than the registry).
    #[allow(dead_code)] // read by the retirement test and the future panel
    fn introduced_in(&self) -> &'static str;
    /// Version after which the migration should be deleted, if scheduled.
    /// `retirement_dates_are_in_the_future` fails once the crate version
    /// reaches it, as a reminder.
    #[allow(dead_code)] // read by the retirement test and the future panel
    fn retire_after(&self) -> Option<&'static str> {
        None
    }
    /// Rewrites files Codex writes while running (session JSONL, state DB).
    fn touches_codex_sessions(&self) -> bool;
    fn run(&self, ctx: &MigrationContext<'_>) -> Result<MigrationOutcome, AppError>;
}

struct CodexThirdPartyBucket;

impl Migration for CodexThirdPartyBucket {
    fn id(&self) -> &'static str {
        "codex-third-party-history-provider-bucket"
    }
    fn introduced_in(&self) -> &'static str {
        "3.17.0"
    }
    fn retire_after(&self) -> Option<&'static str> {
        Some("4.0.0")
    }
    fn touches_codex_sessions(&self) -> bool {
        true
    }
    fn run(&self, ctx: &MigrationContext<'_>) -> Result<MigrationOutcome, AppError> {
        let outcome =
            crate::codex_history_migration::maybe_migrate_codex_third_party_history_provider_bucket(
                ctx.db,
            )?;
        Ok(match outcome.skipped_reason {
            Some(reason) => MigrationOutcome::Skipped(reason),
            None => MigrationOutcome::Applied(format!(
                "sources={}, jsonl_files={}, state_rows={}",
                outcome.source_provider_ids.len(),
                outcome.migrated_jsonl_files,
                outcome.migrated_state_rows
            )),
        })
    }
}

struct CodexTemplateBucket;

impl Migration for CodexTemplateBucket {
    fn id(&self) -> &'static str {
        "codex-provider-template-bucket"
    }
    fn introduced_in(&self) -> &'static str {
        "3.17.0"
    }
    fn touches_codex_sessions(&self) -> bool {
        false
    }
    fn run(&self, ctx: &MigrationContext<'_>) -> Result<MigrationOutcome, AppError> {
        let outcome =
            crate::codex_history_migration::maybe_migrate_codex_provider_template_bucket(ctx.db)?;
        Ok(match outcome.skipped_reason {
            Some(reason) => MigrationOutcome::Skipped(reason),
            None if outcome.migrated_provider_ids.is_empty() => {
                MigrationOutcome::Skipped("no_providers".to_string())
            }
            None => MigrationOutcome::Applied(format!(
                "providers={}",
                outcome.migrated_provider_ids.len()
            )),
        })
    }
}

/// Not a one-shot: retries the unify-history migration when the switch is on
/// but an earlier run did not finish (the function gates itself).
struct CodexOfficialUnify;

impl Migration for CodexOfficialUnify {
    fn id(&self) -> &'static str {
        "codex-official-history-unify"
    }
    fn introduced_in(&self) -> &'static str {
        "3.17.0"
    }
    fn touches_codex_sessions(&self) -> bool {
        true
    }
    fn run(&self, _ctx: &MigrationContext<'_>) -> Result<MigrationOutcome, AppError> {
        let outcome =
            crate::codex_history_migration::maybe_migrate_codex_official_history_to_unified_bucket(
            )?;
        Ok(match outcome.skipped_reason {
            Some(reason) => MigrationOutcome::Skipped(reason),
            None => MigrationOutcome::Applied(format!(
                "jsonl_files={}, state_rows={}",
                outcome.migrated_jsonl_files, outcome.migrated_state_rows
            )),
        })
    }
}

/// Every startup migration, in run order.
pub fn registry() -> &'static [&'static dyn Migration] {
    &[
        &CodexThirdPartyBucket,
        &CodexTemplateBucket,
        &CodexOfficialUnify,
    ]
}

/// Run `migrations` in order. Session-touching ones are deferred while
/// Codex looks active; a failure does not stop the others.
pub fn run_migrations(
    migrations: &[&dyn Migration],
    ctx: &MigrationContext<'_>,
) -> Vec<(&'static str, MigrationReport)> {
    migrations
        .iter()
        .map(|migration| {
            let report = if ctx.codex_active && migration.touches_codex_sessions() {
                MigrationReport::Deferred("codex_active")
            } else {
                match migration.run(ctx) {
                    Ok(outcome) => MigrationReport::Ran(outcome),
                    Err(e) => MigrationReport::Failed(e.to_string()),
                }
            };
            (migration.id(), report)
        })
        .collect()
}

/// Run the registered migrations and log each result. Blocking.
pub fn run_pending(db: &Database) -> Vec<(&'static str, MigrationReport)> {
    let ctx = MigrationContext {
        db,
        codex_active: codex_appears_active(),
    };
    let reports = run_migrations(registry(), &ctx);
    for (id, report) in &reports {
        match report {
            MigrationReport::Ran(MigrationOutcome::Applied(summary)) => {
                log::info!("✓ Migration {id} completed: {summary}")
            }
            MigrationReport::Ran(MigrationOutcome::Skipped(reason)) => {
                log::debug!("○ Migration {id} skipped: {reason}")
            }
            MigrationReport::Deferred(reason) => {
                log::info!("○ Migration {id} deferred to a later startup: {reason}")
            }
            MigrationReport::Failed(error) => log::warn!("✗ Migration {id} failed: {error}"),
        }
    }
    reports
}

/// How recently a Codex state DB write-ahead log must have changed for
/// Codex to count as running.
const CODEX_ACTIVE_WINDOW: Duration = Duration::from_secs(120);

/// Codex keeps its state DB open in WAL mode while it runs, so a `-wal`
/// file modified in the last two minutes means it is (or just was) active.
pub fn codex_appears_active() -> bool {
    let config_dir = crate::codex_config::get_codex_config_dir();
    let config_text = std::fs::read_to_string(config_dir.join("config.toml")).unwrap_or_default();
    crate::codex_state_db::codex_state_db_paths(&config_dir, &config_text)
        .iter()
        .any(|db| wal_recently_modified(db, SystemTime::now(), CODEX_ACTIVE_WINDOW))
}

fn wal_recently_modified(db_path: &Path, now: SystemTime, window: Duration) -> bool {
    let mut wal = db_path.as_os_str().to_owned();
    wal.push("-wal");
    std::fs::metadata(Path::new(&wal))
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() < window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Probe {
        id: &'static str,
        sessions: bool,
        runs: AtomicUsize,
        fail: bool,
    }

    impl Migration for Probe {
        fn id(&self) -> &'static str {
            self.id
        }
        fn introduced_in(&self) -> &'static str {
            "0.0.0"
        }
        fn touches_codex_sessions(&self) -> bool {
            self.sessions
        }
        fn run(&self, _ctx: &MigrationContext<'_>) -> Result<MigrationOutcome, AppError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(AppError::Message("boom".into()))
            } else {
                Ok(MigrationOutcome::Applied("ok".into()))
            }
        }
    }

    fn probe(id: &'static str, sessions: bool, fail: bool) -> Probe {
        Probe {
            id,
            sessions,
            runs: AtomicUsize::new(0),
            fail,
        }
    }

    #[test]
    fn runner_defers_session_migrations_while_codex_is_active() {
        let db = Database::memory().unwrap();
        let sessions = probe("sessions", true, false);
        let templates = probe("templates", false, false);
        let ctx = MigrationContext {
            db: &db,
            codex_active: true,
        };
        let reports = run_migrations(&[&sessions, &templates], &ctx);
        assert_eq!(sessions.runs.load(Ordering::SeqCst), 0);
        assert_eq!(templates.runs.load(Ordering::SeqCst), 1);
        assert_eq!(reports[0].1, MigrationReport::Deferred("codex_active"));
        assert_eq!(
            reports[1].1,
            MigrationReport::Ran(MigrationOutcome::Applied("ok".into()))
        );
    }

    #[test]
    fn runner_continues_after_a_failure() {
        let db = Database::memory().unwrap();
        let failing = probe("a", false, true);
        let next = probe("b", true, false);
        let ctx = MigrationContext {
            db: &db,
            codex_active: false,
        };
        let reports = run_migrations(&[&failing, &next], &ctx);
        assert!(matches!(reports[0].1, MigrationReport::Failed(_)));
        assert_eq!(next.runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn recent_wal_counts_as_active() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state_5.sqlite");
        assert!(!wal_recently_modified(
            &db,
            SystemTime::now(),
            CODEX_ACTIVE_WINDOW
        ));
        std::fs::write(dir.path().join("state_5.sqlite-wal"), b"x").unwrap();
        assert!(wal_recently_modified(
            &db,
            SystemTime::now(),
            CODEX_ACTIVE_WINDOW
        ));
        let later = SystemTime::now() + Duration::from_secs(600);
        assert!(!wal_recently_modified(&db, later, CODEX_ACTIVE_WINDOW));
    }

    #[test]
    fn registry_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for migration in registry() {
            assert!(seen.insert(migration.id()), "duplicate {}", migration.id());
        }
    }

    fn version_tuple(v: &str) -> (u64, u64, u64) {
        let mut parts = v.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    }

    /// Reminder: once the crate reaches a migration's `retire_after`
    /// version, delete the migration (and this list entry).
    #[test]
    fn retirement_dates_are_in_the_future() {
        let current = version_tuple(env!("CARGO_PKG_VERSION"));
        for migration in registry() {
            assert!(version_tuple(migration.introduced_in()) <= current);
            if let Some(retire) = migration.retire_after() {
                assert!(
                    current < version_tuple(retire),
                    "migration {} is due for retirement (retire_after {retire})",
                    migration.id()
                );
            }
        }
    }
}
