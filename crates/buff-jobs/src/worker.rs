//! [`Worker`] — drains a [`crate::Queue`] and dispatches each job to a
//! user-supplied handler.
//!
//! The worker is synchronous (no async runtime needed for the MVP).
//! A future async `Worker.run_async` (tokio-based) is v1.18+ per the
//! T35 task spec.
//!
//! # Retry + dead-letter routing
//!
//! For each dequeued job, the worker invokes the handler. If the
//! handler returns `Ok(())`, the job is ack-completed. If the handler
//! returns `Err(reason)`:
//!
//! 1. The job's attempt counter is incremented via
//!    [`crate::Job::mark_failed`].
//! 2. If attempts remain (`attempts <= max_retries`), the job is
//!    re-enqueued for retry via [`crate::Queue::reenqueue_for_retry`].
//! 3. If the retry budget is exhausted (`attempts > max_retries`),
//!    the job is routed to the dead-letter queue via
//!    [`crate::Queue::route_to_dead_letter`].
//!
//! Backoff delays are computed via [`crate::Job::next_retry_delay`]
//! and exposed for observability (the MVP does NOT sleep before
//! re-enqueue - the in-memory queue is process-local; a future
//! Redis backend will sleep the worker thread). The per-retry delay
//! is available on the job itself via
//! [`crate::Job::backoff`] + [`crate::Backoff::delay`].

use crate::{Job, JobResult, JobsError, JobsResult, Queue};

/// A worker that drains a [`Queue`] and dispatches each job to a
/// user-supplied handler.
///
/// Constructed via [`Worker::new`]. The worker borrows the queue
/// (cheap `Arc` clone) and runs to completion via [`Worker::run`].
#[derive(Debug, Clone)]
pub struct Worker {
    queue: Queue,
}

/// Per-run worker statistics. Returned by [`Worker::run`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkerStats {
    /// Number of jobs the worker pulled from the queue.
    pub processed: u64,
    /// Number of jobs whose handler returned `Ok(())`.
    pub succeeded: u64,
    /// Number of jobs whose handler returned `Err(reason)` at least
    /// once. A job that fails-then-succeeds-on-retry increments both
    /// `failed` and `succeeded`.
    pub failed: u64,
    /// Number of jobs routed to the dead-letter queue (handler
    /// returned `Err(reason)` and the retry budget was exhausted).
    pub dead_lettered: u64,
    /// Number of jobs re-enqueued for retry (subset of `failed`).
    pub retried: u64,
}

impl std::fmt::Display for WorkerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WorkerStats(processed={}, succeeded={}, failed={}, retried={}, dead_lettered={})",
            self.processed, self.succeeded, self.failed, self.retried, self.dead_lettered
        )
    }
}

impl Worker {
    /// Construct a worker that drains the supplied queue.
    pub fn new(queue: Queue) -> Self {
        Self { queue }
    }

    /// Drain the queue: invoke `handler` for each pending job until
    /// the queue is empty. Honors retry budget and routes permanent
    /// failures to the dead-letter queue.
    ///
    /// The handler receives the job by reference and returns
    /// [`JobResult`] (`Result<(), String>`). An `Err(reason)` string
    /// becomes the dead-letter reason if the retry budget is
    /// exhausted.
    ///
    /// Returns [`WorkerStats`] summarising the run.
    pub fn run<F>(&self, mut handler: F) -> JobsResult<WorkerStats>
    where
        F: FnMut(&Job) -> JobResult,
    {
        let mut stats = WorkerStats::default();
        while let Some(mut job) = self.queue.dequeue()? {
            stats.processed = stats.processed.saturating_add(1);
            match handler(&job) {
                Ok(()) => {
                    job.mark_completed();
                    self.queue.ack_completed()?;
                    stats.succeeded = stats.succeeded.saturating_add(1);
                }
                Err(_) => {
                    stats.failed = stats.failed.saturating_add(1);
                    let can_retry = job.mark_failed();
                    if can_retry {
                        self.queue.reenqueue_for_retry(job)?;
                        stats.retried = stats.retried.saturating_add(1);
                    } else {
                        self.queue.route_to_dead_letter(job)?;
                        stats.dead_lettered = stats.dead_lettered.saturating_add(1);
                    }
                }
            }
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Backoff, Job, Priority};
    use std::time::Duration;

    #[test]
    fn run_drains_and_succeeds() {
        let q = Queue::memory();
        q.enqueue(Job::new("a").unwrap()).unwrap();
        q.enqueue(Job::new("b").unwrap()).unwrap();
        let w = Worker::new(q.clone());
        let stats = w.run(|_| Ok(())).unwrap();
        assert_eq!(stats.processed, 2);
        assert_eq!(stats.succeeded, 2);
        assert_eq!(stats.failed, 0);
        assert_eq!(q.stats().completed, 2);
        assert!(q.is_empty());
    }

    #[test]
    fn run_retries_until_success() {
        let q = Queue::memory();
        q.enqueue(
            Job::new("flaky")
                .unwrap()
                .with_max_retries(3)
                .with_backoff(Backoff::fixed(Duration::ZERO)),
        )
        .unwrap();
        let w = Worker::new(q.clone());
        let mut attempts = 0u32;
        let stats = w.run(|_| {
            attempts = attempts.saturating_add(1);
            if attempts < 2 {
                Err("transient".to_string())
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(stats.succeeded, 1);
        assert!(stats.failed >= 1);
        assert_eq!(q.stats().completed, 1);
        assert!(q.dead_letter().is_empty());
    }

    #[test]
    fn run_routes_to_dead_letter_when_budget_exhausted() {
        let q = Queue::memory();
        q.enqueue(
            Job::new("doomed")
                .unwrap()
                .with_max_retries(2)
                .with_backoff(Backoff::fixed(Duration::ZERO)),
        )
        .unwrap();
        let w = Worker::new(q.clone());
        let stats = w.run(|_| Err("permanent-ish".to_string())).unwrap();
        assert_eq!(stats.processed, 3);
        assert_eq!(stats.succeeded, 0);
        assert_eq!(stats.dead_lettered, 1);
        assert_eq!(q.dead_letter().len(), 1);
    }

    #[test]
    fn priority_ordering_honored() {
        let q = Queue::memory();
        let mut order: Vec<String> = Vec::new();
        q.enqueue(Job::new("low").unwrap().with_priority(Priority::Low))
            .unwrap();
        q.enqueue(
            Job::new("high")
                .unwrap()
                .with_priority(Priority::High),
        )
        .unwrap();
        let w = Worker::new(q.clone());
        w.run(|job| {
            order.push(job.payload().to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(order, vec!["high".to_string(), "low".to_string()]);
    }
}
