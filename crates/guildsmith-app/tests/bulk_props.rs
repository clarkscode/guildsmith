//! T007: for any targets and any injected per-item failure or cancel point, every target ends
//! up in exactly one of succeeded, failed or not attempted (data-model §2, SC-005). Batches hold
//! at most 200 (bans), 100 (bulk delete) or 1 (everything else).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use guildsmith_app::bulk::{
    BAN_BATCH, BatchResult, BulkAction, BulkState, DELETE_BATCH, Failure, Hooks, PREVIEW_LIMIT, Problem, Progress,
    SINGLE,
};
use proptest::prelude::*;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

#[derive(Debug, Clone)]
struct Case {
    targets: Vec<u32>,
    batch: usize,
    problem_every: usize,
    fail_every: usize,
    fatal_at_batch: Option<usize>,
    cancel_at_batch: Option<usize>,
}

fn case() -> impl Strategy<Value = Case> {
    (
        prop::collection::btree_set(0u32..5_000, 0..700),
        prop::sample::select(vec![BAN_BATCH, DELETE_BATCH, SINGLE, 7]),
        0usize..9,
        0usize..9,
        prop::option::of(0usize..12),
        prop::option::of(0usize..12),
    )
        .prop_map(|(t, batch, problem_every, fail_every, fatal_at_batch, cancel_at_batch)| Case {
            targets: t.into_iter().collect(),
            batch,
            problem_every,
            fail_every,
            fatal_at_batch,
            cancel_at_batch,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn every_target_is_counted_exactly_once(c in case()) {
        let mut action = BulkAction::new("act", c.targets.clone(), c.batch);
        for (i, t) in c.targets.iter().enumerate() {
            if c.problem_every > 0 && i % c.problem_every == 0 {
                action = action.with_problem(*t, Problem::AboveBot);
            }
        }
        let preview = action.preview();
        prop_assert_eq!(preview.total, c.targets.len());
        prop_assert!(preview.targets.len() <= PREVIEW_LIMIT);
        prop_assert_eq!(preview.targets.len() + preview.more + preview.problems.len(), c.targets.len());

        let batches = AtomicUsize::new(0);
        let max_batch = AtomicUsize::new(0);
        let cancel_at = c.cancel_at_batch;
        let cancelled = move || cancel_at.is_some_and(|n| batches_seen() >= n);
        // The cancel check reads the batch count through a thread-local mirror.
        thread_local!(static SEEN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) });
        fn batches_seen() -> usize { SEEN.with(std::cell::Cell::get) }
        SEEN.with(|s| s.set(0));
        let progress_calls = Mutex::new(Vec::<Progress>::new());
        let progress = |p: Progress| progress_calls.lock().unwrap().push(p);
        let hooks = Hooks { progress: &progress, cancelled: &cancelled };

        let out = block_on(action.run(
            |batch: Vec<u32>| {
                let n = batches.fetch_add(1, Ordering::SeqCst);
                SEEN.with(|s| s.set(n + 1));
                max_batch.fetch_max(batch.len(), Ordering::SeqCst);
                let fatal = c.fatal_at_batch == Some(n);
                let fail_every = c.fail_every;
                async move {
                    if fatal {
                        return BatchResult::Fatal("lost permission".to_owned());
                    }
                    let (mut succeeded, mut failed) = (Vec::new(), Vec::new());
                    for t in batch {
                        if fail_every > 0 && (t as usize).is_multiple_of(fail_every) {
                            failed.push(Failure { item: t, reason: "injected".to_owned() });
                        } else {
                            succeeded.push(t);
                        }
                    }
                    BatchResult::Done { succeeded, failed }
                }
            },
            &hooks,
        ));

        prop_assert_eq!(out.total(), c.targets.len(), "{}", out.summary());
        let mut seen: Vec<u32> = out.succeeded.iter().copied()
            .chain(out.failed.iter().map(|f| f.item))
            .chain(out.not_attempted.iter().copied())
            .collect();
        seen.sort_unstable();
        let mut want = c.targets.clone();
        want.sort_unstable();
        prop_assert_eq!(seen, want, "each target exactly once");
        prop_assert!(max_batch.load(Ordering::SeqCst) <= c.batch);
        match &out.state {
            BulkState::Completed => prop_assert!(out.not_attempted.is_empty()),
            BulkState::Cancelled => prop_assert!(c.cancel_at_batch.is_some()),
            BulkState::Stopped { error } => prop_assert_eq!(error, "lost permission"),
        }
        let last = progress_calls.lock().unwrap().last().copied();
        if let Some(p) = last {
            prop_assert_eq!(p.total, c.targets.len());
            prop_assert_eq!(p.done, out.succeeded.len() + out.failed.len());
        }
    }
}

#[test]
fn bans_go_in_batches_of_200() {
    let targets: Vec<u32> = (0..450).collect();
    let action = BulkAction::new("ban", targets, BAN_BATCH);
    assert_eq!(action.estimated_requests(), 3);
    let sizes = Mutex::new(Vec::new());
    let out = block_on(action.run(
        |b: Vec<u32>| {
            sizes.lock().unwrap().push(b.len());
            async move { BatchResult::Done { succeeded: b, failed: Vec::new() } }
        },
        &Hooks::none(),
    ));
    assert_eq!(*sizes.lock().unwrap(), vec![200, 200, 50]);
    assert_eq!(out.succeeded.len(), 450);
    assert_eq!(out.state, BulkState::Completed);
}

#[test]
fn problems_fail_without_a_request_and_a_batch_that_omits_items_fails_them() {
    let action = BulkAction::new("kick", vec![1u32, 2, 3], SINGLE).with_problem(2, Problem::Owner);
    let preview = action.preview();
    assert_eq!(preview.targets, vec![1, 3]);
    assert_eq!(preview.problems[0].reason, "the server owner can't be moderated");
    let calls = AtomicUsize::new(0);
    let out = block_on(action.run(
        |b: Vec<u32>| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                // Report nothing for item 3.
                if b == [1] {
                    BatchResult::single(1, Ok(()))
                } else {
                    BatchResult::Done { succeeded: vec![], failed: vec![] }
                }
            }
        },
        &Hooks::none(),
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2, "the owner costs no request");
    assert_eq!(out.succeeded, vec![1]);
    assert_eq!(out.failed.len(), 2);
    assert_eq!(out.total(), 3);
}
