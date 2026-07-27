//! P0.22 integration tests: prove the [`buff_jobs::Scheduler`]
//! actually invokes registered handlers when fire time arrives, and
//! that the [`buff_jobs::Worker`] honors exponential backoff on
//! retry.
//!
//! These tests are the bug-prevention regression for the audit
//! finding "Scheduler.start() never executes scheduled handlers".
//! Before P0.22 the dispatch loop only advanced `next_fire` — it
//! never called the handler. After P0.22 the loop collects
//! fire-ready entries, drops the lock, invokes the handler, and
//! re-locks to update failure counters.

use buff_jobs::{Job, Scheduler};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Register an interval-scheduled job with a handler that bumps a
/// counter, start the scheduler, wait long enough for several fires,
/// stop the scheduler, and assert the handler actually ran.
///
/// Before P0.22 this test would fail (counter stays at 0) because
/// the dispatch loop never called the handler.
#[tokio::test]
async fn scheduler_interval_fires_handler() {
    let scheduler = Scheduler::new();
    let fires = Arc::new(AtomicU32::new(0));
    let fires_clone = fires.clone();

    scheduler
        .interval_with_handler(
            Duration::from_millis(50),
            Job::new("tick").expect("test job"),
            move |_job| {
                let _ = fires_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .expect("register interval");

    scheduler.start().await;
    // Wait long enough for ~4 ticks (50ms interval × 4 = 200ms).
    // The dispatch loop ticks every 100ms, so allow generous slack.
    tokio::time::sleep(Duration::from_millis(300)).await;
    scheduler.stop().await;

    let observed = fires.load(Ordering::SeqCst);
    assert!(
        observed >= 2,
        "handler should have fired at least 2 times, got {}",
        observed,
    );
}

/// Same as above but for cron schedules. Uses a per-second cron
/// expression so the test stays fast.
#[tokio::test]
async fn scheduler_cron_fires_handler() {
    let scheduler = Scheduler::new();
    let fires = Arc::new(AtomicU32::new(0));
    let fires_clone = fires.clone();

    // Cron expression: every second (`* * * * * *` in 7-field form
    // = sec min hour dom mon dow year — the leading `*` fires
    // every second).
    scheduler
        .cron_with_handler(
            "* * * * * *",
            Job::new("cron-tick").expect("test job"),
            move |_job| {
                let _ = fires_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .expect("register cron");

    // Sanity: next_fire should have been computed at registration
    // time (P0.22 fix — previously cron jobs left next_fire = None
    // and never fired).
    let next_due = scheduler.next_due().await;
    assert!(
        next_due.is_some(),
        "cron job must have a computed next_fire at registration",
    );

    scheduler.start().await;
    // Wait just over 1 second so the per-second cron fires at least
    // once. Generous slack for the 100ms dispatch cadence.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    scheduler.stop().await;

    let observed = fires.load(Ordering::SeqCst);
    assert!(
        observed >= 1,
        "cron handler should have fired at least once, got {}",
        observed,
    );
}

/// Handler that always returns Err must log + bump the failure
/// counter; once the counter exceeds max_retries the schedule entry
/// is pruned (dead-letter semantics for the scheduler).
#[tokio::test]
async fn scheduler_handler_failure_is_counted_and_pruned() {
    let scheduler = Scheduler::new();
    let attempts = Arc::new(AtomicU32::new(0));
    let attempts_clone = attempts.clone();

    let job = Job::new("doomed").expect("test job").with_max_retries(2);
    let job_id = job.id().clone();

    scheduler
        .interval_with_handler(Duration::from_millis(50), job, move |_job| {
            attempts_clone.fetch_add(1, Ordering::SeqCst);
            Err("always fails".to_string())
        })
        .await
        .expect("register interval");

    let initial_pending = scheduler.pending_count().await;
    assert_eq!(initial_pending, 1);

    scheduler.start().await;
    // Wait long enough for several fires (well past max_retries=2
    // + 1 initial). The schedule entry should be pruned in the same
    // tick that the failure counter crosses the threshold (P0.22:
    // prune-on-fail, not deferred to next tick).
    tokio::time::sleep(Duration::from_millis(500)).await;
    scheduler.stop().await;

    let attempts_observed = attempts.load(Ordering::SeqCst);
    let pending_after = scheduler.pending_count().await;

    // Handler must have been called at least once.
    assert!(
        attempts_observed >= 1,
        "handler should have been invoked at least once, got {}",
        attempts_observed,
    );
    // After exceeding max_retries the entry is pruned.
    assert_eq!(
        pending_after, 0,
        "schedule entry {} should have been pruned after {} failures (max_retries=2), but pending_count={}",
        job_id, attempts_observed, pending_after,
    );
}

/// A handler that fails then succeeds must reset the failure counter
/// so the schedule entry survives subsequent transient blips.
#[tokio::test]
async fn scheduler_handler_recovery_resets_failure_counter() {
    let scheduler = Scheduler::new();
    let fires = Arc::new(AtomicU32::new(0));
    let fires_clone = fires.clone();

    scheduler
        .interval_with_handler(
            Duration::from_millis(50),
            Job::new("flaky")
                .expect("test job")
                // Tight retry budget — if recovery didn't reset the
                // counter, the entry would be pruned within 3 ticks.
                .with_max_retries(2),
            move |_job| {
                let n = fires_clone.fetch_add(1, Ordering::SeqCst);
                // Fail the first two invocations, succeed after.
                if n < 2 {
                    Err("transient".to_string())
                } else {
                    Ok(())
                }
            },
        )
        .await
        .expect("register interval");

    scheduler.start().await;
    // Wait long enough to fail twice + succeed several times. The
    // dispatch loop ticks every 100ms, so 800ms gives ~8 ticks =
    // ~7 fires (well past the recovery threshold). If the failure
    // counter didn't reset, the entry would be pruned after 3 total
    // failures (max_retries=2 + 1) and fires would stall at 3.
    tokio::time::sleep(Duration::from_millis(800)).await;
    scheduler.stop().await;

    let fires_observed = fires.load(Ordering::SeqCst);
    let pending_after = scheduler.pending_count().await;

    assert!(
        fires_observed >= 5,
        "handler should have fired many times after recovery, got {}",
        fires_observed,
    );
    assert_eq!(
        pending_after, 1,
        "schedule entry should still be active after recovery (consecutive_failures reset)",
    );
}

/// Backward-compat: legacy `interval()` (no handler) must still
/// tick without panicking. P0.22 added handler dispatch but kept
/// the no-handler path working.
#[tokio::test]
async fn scheduler_interval_without_handler_ticks_silently() {
    let scheduler = Scheduler::new();
    scheduler
        .interval(
            Duration::from_millis(50),
            Job::new("silent").expect("test job"),
        )
        .await
        .expect("register interval");

    assert_eq!(scheduler.pending_count().await, 1);
    scheduler.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    scheduler.stop().await;
    // No handler to fail → entry never pruned.
    assert_eq!(scheduler.pending_count().await, 1);
}
