//! Structured traces for the critical state machines (SVC-23).
//!
//! An [`Operation`] covers one user-visible action (a provider switch, a proxy
//! takeover toggle, a cloud-sync upload/download). It
//!
//! - opens a `tracing` span named after the operation with its fields (so a
//!   tracing subscriber, when one is installed, sees one span with child
//!   events),
//! - writes `[op#<id> <kind>] ...` lines to the regular log for start, every
//!   step and the outcome, so the log file ties the sub-steps of one switch
//!   together even without a subscriber,
//! - keeps the last [`MAX_RECENT_OPERATIONS`] finished operations in memory
//!   ([`recent_operations`]) for a support view.
//!
//! Synchronous operations ([`Operation::start`]) collect steps recorded with
//! [`step`] anywhere below them on the same thread. Operations that cross
//! `.await` points ([`Operation::start_async`]) record steps through
//! [`Operation::step`] only. Field values must not contain secrets: pass ids
//! and app names only.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

/// How many finished operations [`recent_operations`] keeps.
pub(crate) const MAX_RECENT_OPERATIONS: usize = 200;
/// Steps kept per operation (later ones are counted, not stored).
const MAX_STEPS_PER_OPERATION: usize = 64;

/// One sub-step of an operation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationStep {
    pub name: String,
    /// `ok`, `skipped`, `warning` or `error`.
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Milliseconds since the operation started.
    pub at_ms: u64,
}

/// A finished operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub id: u64,
    pub kind: &'static str,
    pub fields: Vec<(String, String)>,
    /// Unix milliseconds.
    pub started_at: i64,
    pub duration_ms: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub steps: Vec<OperationStep>,
    pub dropped_steps: usize,
}

/// Outcome of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepOutcome {
    Ok,
    Skipped,
    Warning,
    Error,
}

impl StepOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Skipped => "skipped",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

struct Shared {
    id: u64,
    kind: &'static str,
    started: Instant,
    steps: Mutex<Vec<OperationStep>>,
    dropped_steps: AtomicUsize,
}

impl Shared {
    fn record(&self, name: &str, outcome: StepOutcome, detail: Option<&str>) {
        let outcome = outcome.as_str();
        let at_ms = self.started.elapsed().as_millis() as u64;
        let (id, kind) = (self.id, self.kind);
        tracing::info!(op = id, step = name, outcome, detail, "operation step");
        match detail {
            Some(detail) if outcome != "ok" => {
                log::warn!("[op#{id} {kind}] {name}: {outcome}: {detail}")
            }
            Some(detail) => log::info!("[op#{id} {kind}] {name}: {outcome} ({detail})"),
            None if outcome == "ok" => log::debug!("[op#{id} {kind}] {name}: ok"),
            None => log::info!("[op#{id} {kind}] {name}: {outcome}"),
        }
        let Ok(mut steps) = self.steps.lock() else {
            return;
        };
        if steps.len() < MAX_STEPS_PER_OPERATION {
            steps.push(OperationStep {
                name: name.to_string(),
                outcome: outcome.to_string(),
                detail: detail.map(str::to_string),
                at_ms,
            });
        } else {
            self.dropped_steps.fetch_add(1, Ordering::Relaxed);
        }
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static RECENT: Mutex<VecDeque<OperationRecord>> = Mutex::new(VecDeque::new());

thread_local! {
    /// Synchronous operations open on this thread, innermost last.
    static ACTIVE: RefCell<Vec<Arc<Shared>>> = const { RefCell::new(Vec::new()) };
}

/// An operation in progress; finish it with [`Operation::finish`]. Dropping it
/// unfinished records it as failed ("not finished").
pub(crate) struct Operation {
    shared: Arc<Shared>,
    fields: Vec<(String, String)>,
    started_at: i64,
    span: tracing::Span,
    /// Registered as this thread's current operation (sync operations only).
    on_thread: bool,
    finished: bool,
}

impl Operation {
    /// Start a synchronous operation of `kind` with descriptive `fields` (no
    /// secrets). Until it finishes, [`step`] calls on this thread attach to it.
    pub(crate) fn start(kind: &'static str, fields: &[(&str, &str)]) -> Self {
        let op = Self::new(kind, fields, true);
        ACTIVE.with(|active| active.borrow_mut().push(Arc::clone(&op.shared)));
        op
    }

    /// Start an operation that runs across `.await` points (and may resume on
    /// another thread): steps are recorded with [`Operation::step`] only.
    pub(crate) fn start_async(kind: &'static str, fields: &[(&str, &str)]) -> Self {
        Self::new(kind, fields, false)
    }

    fn new(kind: &'static str, fields: &[(&str, &str)], on_thread: bool) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let fields: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let rendered = render_fields(&fields);
        let span = tracing::info_span!("operation", op = id, kind = kind, fields = %rendered);
        log::info!("[op#{id} {kind}] start {rendered}");
        Self {
            shared: Arc::new(Shared {
                id,
                kind,
                started: Instant::now(),
                steps: Mutex::new(Vec::new()),
                dropped_steps: AtomicUsize::new(0),
            }),
            fields,
            started_at: chrono::Utc::now().timestamp_millis(),
            span,
            on_thread,
            finished: false,
        }
    }

    #[cfg(test)]
    fn id(&self) -> u64 {
        self.shared.id
    }

    /// The tracing span of this operation (for `tracing::Instrument`).
    pub(crate) fn span(&self) -> tracing::Span {
        self.span.clone()
    }

    /// Run `f` inside the operation's tracing span.
    pub(crate) fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T {
        self.span.in_scope(f)
    }

    /// Record a step of this operation.
    pub(crate) fn step(&self, name: &str, outcome: StepOutcome, detail: Option<&str>) {
        self.shared.record(name, outcome, detail);
    }

    /// Record a step from a `Result`: `ok`, or `error` with its message.
    pub(crate) fn step_result<T, E: std::fmt::Display>(&self, name: &str, result: &Result<T, E>) {
        match result {
            Ok(_) => self.step(name, StepOutcome::Ok, None),
            Err(e) => self.step(name, StepOutcome::Error, Some(&e.to_string())),
        }
    }

    /// Record the outcome, log it and store the operation in the ring buffer.
    pub(crate) fn finish<T, E: std::fmt::Display>(mut self, result: &Result<T, E>) {
        let error = result.as_ref().err().map(|e| e.to_string());
        self.complete(error);
    }

    fn complete(&mut self, error: Option<String>) {
        self.finished = true;
        let (id, kind) = (self.shared.id, self.shared.kind);
        if self.on_thread {
            ACTIVE.with(|active| {
                let mut active = active.borrow_mut();
                if let Some(pos) = active.iter().rposition(|op| op.id == id) {
                    active.remove(pos);
                }
            });
        }
        let duration_ms = self.shared.started.elapsed().as_millis() as u64;
        let steps = self
            .shared
            .steps
            .lock()
            .map(|mut steps| std::mem::take(&mut *steps))
            .unwrap_or_default();
        let dropped_steps = self.shared.dropped_steps.load(Ordering::Relaxed);
        let _entered = self.span.enter();
        match &error {
            None => {
                tracing::info!(op = id, duration_ms, "operation finished");
                log::info!(
                    "[op#{id} {kind}] ok in {duration_ms} ms ({} steps)",
                    steps.len()
                );
            }
            Some(e) => {
                tracing::warn!(op = id, duration_ms, error = %e, "operation failed");
                log::warn!("[op#{id} {kind}] failed after {duration_ms} ms: {e}");
            }
        }
        let record = OperationRecord {
            id,
            kind,
            fields: std::mem::take(&mut self.fields),
            started_at: self.started_at,
            duration_ms,
            ok: error.is_none(),
            error,
            steps,
            dropped_steps,
        };
        if let Ok(mut recent) = RECENT.lock() {
            push_bounded(&mut recent, record, MAX_RECENT_OPERATIONS);
        }
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            self.complete(Some("not finished".to_string()));
        }
    }
}

/// Record a step of the innermost synchronous operation open on this thread.
pub(crate) fn step(name: &str, outcome: StepOutcome, detail: Option<&str>) {
    let current = ACTIVE.with(|active| active.borrow().last().cloned());
    match current {
        Some(op) => op.record(name, outcome, detail),
        None => log::debug!(
            "{name}: {}{}",
            outcome.as_str(),
            detail.map(|d| format!(" ({d})")).unwrap_or_default()
        ),
    }
}

/// Record a step from a `Result`: `ok`, or `error` with its message.
pub(crate) fn step_result<T, E: std::fmt::Display>(name: &str, result: &Result<T, E>) {
    match result {
        Ok(_) => step(name, StepOutcome::Ok, None),
        Err(e) => step(name, StepOutcome::Error, Some(&e.to_string())),
    }
}

/// The finished operations, oldest first (at most [`MAX_RECENT_OPERATIONS`]).
/// For a support view; the IPC command exposing it is not wired yet.
#[cfg_attr(not(test), allow(dead_code))]
pub fn recent_operations() -> Vec<OperationRecord> {
    RECENT
        .lock()
        .map(|recent| recent.iter().cloned().collect())
        .unwrap_or_default()
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T, cap: usize) {
    while queue.len() >= cap {
        queue.pop_front();
    }
    queue.push_back(item);
}

fn render_fields(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: u64) -> OperationRecord {
        recent_operations()
            .into_iter()
            .find(|r| r.id == id)
            .expect("operation recorded")
    }

    #[test]
    fn operation_records_steps_and_outcome() {
        let op = Operation::start("switch", &[("app", "claude"), ("to", "p2")]);
        let id = op.id();
        op.in_scope(|| {
            step("backfill", StepOutcome::Ok, None);
            step("mcp_sync", StepOutcome::Warning, Some("gemini: denied"));
        });
        op.finish(&Ok::<(), String>(()));

        let record = find(id);
        assert!(record.ok);
        assert_eq!(record.kind, "switch");
        assert_eq!(
            record.fields,
            vec![
                ("app".to_string(), "claude".to_string()),
                ("to".to_string(), "p2".to_string())
            ]
        );
        let names: Vec<&str> = record.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["backfill", "mcp_sync"]);
        assert_eq!(record.steps[1].outcome, "warning");
        // Steps after the operation do not attach to it.
        step("orphan", StepOutcome::Ok, None);
        assert_eq!(find(id).steps.len(), 2);
    }

    #[test]
    fn nested_operations_and_unfinished_drop() {
        let outer = Operation::start("sync", &[("direction", "download")]);
        let inner = Operation::start("switch", &[]);
        step("inner_step", StepOutcome::Ok, None);
        let inner_id = inner.id();
        drop(inner);
        step("outer_step", StepOutcome::Ok, None);
        let outer_id = outer.id();
        outer.finish(&Err::<(), _>("boom"));

        let inner = find(inner_id);
        assert!(!inner.ok);
        assert_eq!(inner.error.as_deref(), Some("not finished"));
        assert_eq!(inner.steps.len(), 1);
        let outer = find(outer_id);
        assert_eq!(outer.error.as_deref(), Some("boom"));
        assert_eq!(outer.steps[0].name, "outer_step");
    }

    #[test]
    fn async_operation_records_its_own_steps_only() {
        let op = Operation::start_async("sync", &[("transport", "S3")]);
        op.step("manifest", StepOutcome::Ok, None);
        // A free-standing step on this thread does not attach to it.
        step("unrelated", StepOutcome::Ok, None);
        op.step_result("upload", &Err::<(), _>("412"));
        let id = op.id();
        op.finish(&Err::<(), _>("conflict"));
        let record = find(id);
        let names: Vec<&str> = record.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["manifest", "upload"]);
        assert_eq!(record.steps[1].detail.as_deref(), Some("412"));
    }

    #[test]
    fn ring_buffer_is_bounded_and_steps_are_capped() {
        let op = Operation::start("bulk", &[]);
        for i in 0..(MAX_STEPS_PER_OPERATION + 5) {
            step(&format!("s{i}"), StepOutcome::Ok, None);
        }
        let id = op.id();
        op.finish(&Ok::<(), String>(()));
        let record = find(id);
        assert_eq!(record.steps.len(), MAX_STEPS_PER_OPERATION);
        assert_eq!(record.dropped_steps, 5);

        let mut queue = VecDeque::new();
        for i in 0..(MAX_RECENT_OPERATIONS + 3) {
            push_bounded(&mut queue, i, MAX_RECENT_OPERATIONS);
        }
        assert_eq!(queue.len(), MAX_RECENT_OPERATIONS);
        assert_eq!(queue.front(), Some(&3));
    }
}
