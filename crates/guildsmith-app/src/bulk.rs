//! Bulk actions (FR-005, data-model §2, research E10).
//!
//! A bulk action previews its exact targets (or the first 50 and a total) with any per-item
//! problems, then runs in batches through the rate limiter, reporting progress and stopping
//! between batches when cancelled. Every target ends up in exactly one of `succeeded`,
//! `failed` (with a reason) or `not_attempted` (SC-005).
//!
//! States: `Previewed → Running → (Completed | Cancelled | Stopped(error))`.

use std::future::Future;

use schemars::JsonSchema;
use serde::Serialize;

/// Bulk ban takes at most 200 users per request.
pub const BAN_BATCH: usize = 200;
/// Bulk delete takes at most 100 messages per request.
pub const DELETE_BATCH: usize = 100;
/// Everything else goes one item per request.
pub const SINGLE: usize = 1;
/// A preview lists at most this many targets, plus the total.
pub const PREVIEW_LIMIT: usize = 50;

/// Why a target won't be acted on. Found before running, so it costs no request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// The member's highest role is at or above the bot's.
    AboveBot,
    /// The server owner can't be moderated.
    Owner,
    /// The bot won't act on itself.
    IsSelf,
    /// Too old for bulk deletion (over 14 days).
    TooOld,
    /// Not in a voice channel, so voice actions don't apply.
    NotInVoice,
    /// Anything else, in words.
    Other(String),
}

impl Problem {
    /// The reason shown in previews and reports.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Problem::AboveBot => "their highest role is at or above the bot's".to_owned(),
            Problem::Owner => "the server owner can't be moderated".to_owned(),
            Problem::IsSelf => "the bot won't act on itself".to_owned(),
            Problem::TooOld => "older than 14 days, too old for bulk deletion".to_owned(),
            Problem::NotInVoice => "not in a voice channel".to_owned(),
            Problem::Other(s) => s.clone(),
        }
    }
}

/// A target with a problem, as previews and reports show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Failure<T> {
    pub item: T,
    pub reason: String,
}

/// What a bulk action would do. `preview` is always `true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BulkPreview<T> {
    pub preview: bool,
    /// e.g. `ban`.
    pub action: String,
    /// Every target, problems included.
    pub total: usize,
    /// The first 50 targets that would be acted on.
    pub targets: Vec<T>,
    /// How many more targets aren't listed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more: usize,
    /// Targets that will be skipped, and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Failure<T>>,
    pub batch_size: usize,
    pub estimated_requests: usize,
    /// Extra lines, e.g. what else happens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BulkState {
    Completed,
    /// Stopped between batches because the caller cancelled.
    Cancelled,
    /// Stopped by an error that makes the rest pointless (e.g. lost permission).
    Stopped {
        error: String,
    },
}

/// What a run did with every target (SC-005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BulkOutcome<T> {
    #[serde(flatten)]
    pub state: BulkState,
    pub succeeded: Vec<T>,
    pub failed: Vec<Failure<T>>,
    pub not_attempted: Vec<T>,
}

impl<T> BulkOutcome<T> {
    /// Every target counted once.
    #[must_use]
    pub fn total(&self) -> usize {
        self.succeeded.len() + self.failed.len() + self.not_attempted.len()
    }

    /// `3 succeeded, 1 failed, 0 not attempted`.
    #[must_use]
    pub fn summary(&self) -> String {
        let head = format!(
            "{} succeeded, {} failed, {} not attempted",
            self.succeeded.len(),
            self.failed.len(),
            self.not_attempted.len()
        );
        match &self.state {
            BulkState::Completed => head,
            BulkState::Cancelled => format!("cancelled: {head}"),
            BulkState::Stopped { error } => format!("stopped ({error}): {head}"),
        }
    }
}

/// What one batch request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchResult<T> {
    /// Per item: done, or failed with a reason. The run goes on.
    Done { succeeded: Vec<T>, failed: Vec<Failure<T>> },
    /// The whole batch failed in a way that stops the run; the batch counts as failed.
    Fatal(String),
}

impl<T> BatchResult<T> {
    /// One item's result, for one-at-a-time actions.
    pub fn single(item: T, res: Result<(), String>) -> Self {
        match res {
            Ok(()) => BatchResult::Done { succeeded: vec![item], failed: Vec::new() },
            Err(reason) => BatchResult::Done { succeeded: Vec::new(), failed: vec![Failure { item, reason }] },
        }
    }
}

/// Progress after each batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Targets handled so far (succeeded or failed), problems included.
    pub done: usize,
    pub total: usize,
}

/// How a caller follows and stops a run: MCP progress and cancellation notifications, or
/// stderr and Ctrl-C for the CLI.
pub struct Hooks<'a> {
    pub progress: &'a (dyn Fn(Progress) + Send + Sync),
    pub cancelled: &'a (dyn Fn() -> bool + Send + Sync),
}

impl Hooks<'static> {
    /// No progress reports, never cancelled.
    #[must_use]
    pub fn none() -> Self {
        Hooks { progress: &|_| {}, cancelled: &|| false }
    }
}

/// A previewable action over many targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkAction<T> {
    action: String,
    targets: Vec<T>,
    problems: Vec<(T, Problem)>,
    batch_size: usize,
    notes: Vec<String>,
}

impl<T: Clone + PartialEq> BulkAction<T> {
    /// `batch_size` is 200 (bans), 100 (bulk delete) or 1.
    pub fn new(action: impl Into<String>, targets: Vec<T>, batch_size: usize) -> Self {
        BulkAction {
            action: action.into(),
            targets,
            problems: Vec::new(),
            batch_size: batch_size.max(1),
            notes: Vec::new(),
        }
    }

    /// Marks a target as one that will be skipped. Adds it to the targets if it isn't one.
    #[must_use]
    pub fn with_problem(mut self, item: T, problem: Problem) -> Self {
        if !self.targets.contains(&item) {
            self.targets.push(item.clone());
        }
        self.problems.push((item, problem));
        self
    }

    /// Adds a line to the preview.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    fn has_problem(&self, item: &T) -> bool {
        self.problems.iter().any(|(t, _)| t == item)
    }

    /// Targets that will be acted on.
    #[must_use]
    pub fn runnable(&self) -> Vec<T> {
        self.targets.iter().filter(|t| !self.has_problem(t)).cloned().collect()
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.targets.len()
    }

    #[must_use]
    pub fn estimated_requests(&self) -> usize {
        self.runnable().len().div_ceil(self.batch_size)
    }

    #[must_use]
    pub fn preview(&self) -> BulkPreview<T> {
        let runnable = self.runnable();
        let listed: Vec<T> = runnable.iter().take(PREVIEW_LIMIT).cloned().collect();
        BulkPreview {
            preview: true,
            action: self.action.clone(),
            total: self.targets.len(),
            more: runnable.len() - listed.len(),
            targets: listed,
            problems: self.problems.iter().map(|(t, p)| Failure { item: t.clone(), reason: p.reason() }).collect(),
            batch_size: self.batch_size,
            estimated_requests: self.estimated_requests(),
            notes: self.notes.clone(),
        }
    }

    /// Runs the action: problem targets fail without a request, the rest go in batches
    /// through `exec`. Stops between batches when `hooks.cancelled()` says so, or after a
    /// fatal batch.
    pub async fn run<F, Fut>(self, mut exec: F, hooks: &Hooks<'_>) -> BulkOutcome<T>
    where
        F: FnMut(Vec<T>) -> Fut,
        Fut: Future<Output = BatchResult<T>>,
    {
        let total = self.targets.len();
        let runnable = self.runnable();
        let mut out = BulkOutcome {
            state: BulkState::Completed,
            succeeded: Vec::new(),
            failed: self.problems.iter().map(|(t, p)| Failure { item: t.clone(), reason: p.reason() }).collect(),
            not_attempted: Vec::new(),
        };
        let mut batches = runnable.chunks(self.batch_size).map(<[T]>::to_vec);
        while let Some(batch) = batches.next() {
            if (hooks.cancelled)() {
                out.state = BulkState::Cancelled;
                out.not_attempted.extend(batch);
                out.not_attempted.extend(batches.flatten());
                break;
            }
            match exec(batch.clone()).await {
                BatchResult::Done { succeeded, failed } => {
                    // Anything the batch didn't report on counts as failed, so no target is lost.
                    for item in batch {
                        if succeeded.contains(&item) {
                            out.succeeded.push(item);
                        } else if let Some(f) = failed.iter().find(|f| f.item == item) {
                            out.failed.push(f.clone());
                        } else {
                            out.failed.push(Failure { item, reason: "no result for this item".to_owned() });
                        }
                    }
                }
                BatchResult::Fatal(error) => {
                    out.failed.extend(batch.into_iter().map(|item| Failure { item, reason: error.clone() }));
                    out.not_attempted.extend(batches.by_ref().flatten());
                    out.state = BulkState::Stopped { error };
                }
            }
            (hooks.progress)(Progress { done: out.succeeded.len() + out.failed.len(), total });
            if matches!(out.state, BulkState::Stopped { .. }) {
                break;
            }
        }
        out
    }
}
