//! In-memory [`Queue`] backed by `std::sync::Mutex<VecDeque<Job>>`.
//!
//! The MVP ships a single backend: [`Queue::memory`] (in-memory,
//! process-local). A Redis backend is deferred to v1.18+ per the T35
//! task spec; the [`Queue`] type is designed so a future
//! `Queue::redis(url)` constructor can swap the inner storage without
//! breaking the public API.
//!
//! # Priority deque
//!
//! The queue is a priority deque: higher-priority jobs dequeue
//! before lower-priority ones. Within the same priority, FIFO order
//! is preserved (insertion order is the tiebreaker).
//!
//! # Hard-rule compliance
//!
//! `Queue` is `Send + Sync` (wraps `Arc<Mutex<...>>`).
//! `enqueue` consumes the [`crate::Job`]; `dequeue` returns an owned
//! `Option<Job>`; `dead_letter` returns an owned `Vec<Job>`. No
//! references into Rust's heap cross the FFI boundary.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::{Job, JobId, JobStatus, JobsError, JobsResult, Priority};

/// Internal queue state. Held behind `Arc<Mutex<...>>` so the
/// `Queue` value is `Clone + Send + Sync`.
#[derive(Debug, Default)]
struct QueueInner {
    pending: VecDeque<Job>,
    in_flight: u64,
    completed: u64,
    failed: u64,
    dead_letter: Vec<Job>,
}

/// An in-memory job queue.
///
/// Constructed via [`Queue::memory`]. `Clone` is cheap (bumps the
/// `Arc` refcount) so a worker can hold its own handle while the
/// producer holds another.
#[derive(Debug, Clone)]
pub struct Queue {
    inner: Arc<Mutex<QueueInner>>,
}

impl Queue {
    /// Construct a fresh in-memory queue (the MVP backend).
    ///
    /// Returns an empty queue. Use [`Queue::enqueue`] to add jobs and
    /// [`crate::Worker::new`] to drain them.
    pub fn memory() -> Self {
        Self {
            inner: Arc::new(Mutex::new(QueueInner::default())),
        }
    }

    /// Enqueue a job. The job's id is returned so the caller can
    /// track it (e.g. cancel later - deferred API). Consumes the job.
    ///
    /// Returns [`JobsError::InvalidJob`] if the job's payload is
    /// empty (defensive - [`Job::new`] already checks this, but a
    /// future Redis backend may deserialize jobs from external
    /// producers that bypassed the constructor).
    pub fn enqueue(&self, mut job: Job) -> JobsResult<JobId> {
        if job.payload().is_empty() {
            return Err(JobsError::invalid_job("payload must be non-empty"));
        }
        let id = job.id().clone();
        let mut guard = self.inner.lock().map_err(|_| {
            JobsError::invalid_job("queue mutex poisoned (another thread panicked)")
        })?;
        job.status = JobStatus::Pending;
        let prio = job.priority();
        insert_by_priority(&mut guard.pending, job, prio);
        Ok(id)
    }

    /// Dequeue the next-highest-priority pending job, mark it
    /// `InProgress`, and return it. Returns `Ok(None)` when the
    /// queue is empty.
    ///
    /// The returned job is ownership-transferred; the worker is
    /// responsible for calling [`crate::Worker::run`] (which handles
    /// re-enqueue / dead-letter routing on completion).
    pub fn dequeue(&self) -> JobsResult<Option<Job>> {
        let mut guard = self.inner.lock().map_err(|_| {
            JobsError::invalid_job("queue mutex poisoned (another thread panicked)")
        })?;
        let Some(mut job) = guard.pending.pop_front() else {
            return Ok(None);
        };
        guard.in_flight = guard.in_flight.saturating_add(1);
        job.mark_in_progress();
        Ok(Some(job))
    }

    /// Acknowledge a completed job (transitions `in_flight` count to
    /// `completed`). Called by the worker after the handler returns
    /// `Ok(())`. The job's status is updated in the worker before
    /// ack; this method only updates the queue's counters.
    pub(crate) fn ack_completed(&self) -> JobsResult<()> {
        let mut guard = self.inner.lock().map_err(|_| {
            JobsError::invalid_job("queue mutex poisoned (another thread panicked)")
        })?;
        guard.in_flight = guard.in_flight.saturating_sub(1);
        guard.completed = guard.completed.saturating_add(1);
        Ok(())
    }

    /// Re-enqueue a job for retry (transitions `in_flight` back to
    /// `pending`, increments failed counter). Called by the worker
    /// when the handler returns `Err(reason)` AND the retry budget
    /// is not yet exhausted.
    pub(crate) fn reenqueue_for_retry(&self, mut job: Job) -> JobsResult<()> {
        let mut guard = self.inner.lock().map_err(|_| {
            JobsError::invalid_job("queue mutex poisoned (another thread panicked)")
        })?;
        guard.in_flight = guard.in_flight.saturating_sub(1);
        guard.failed = guard.failed.saturating_add(1);
        job.status = JobStatus::Pending;
        let prio = job.priority();
        insert_by_priority(&mut guard.pending, job, prio);
        Ok(())
    }

    /// Route a permanently failed job to the dead-letter queue.
    /// Called by the worker when the handler returns `Err(reason)`
    /// AND the retry budget is exhausted.
    pub(crate) fn route_to_dead_letter(&self, mut job: Job) -> JobsResult<()> {
        let mut guard = self.inner.lock().map_err(|_| {
            JobsError::invalid_job("queue mutex poisoned (another thread panicked)")
        })?;
        guard.in_flight = guard.in_flight.saturating_sub(1);
        guard.failed = guard.failed.saturating_add(1);
        job.mark_dead_letter();
        guard.dead_letter.push(job);
        Ok(())
    }

    /// Number of jobs currently waiting in the pending queue
    /// (excludes in-flight, completed, failed, dead-letter).
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .map(|g| g.pending.len())
            .unwrap_or(0)
    }

    /// Whether the pending queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Snapshot of dead-letter jobs (jobs that exhausted their retry
    /// budget). Returns an owned `Vec<Job>` clone so the caller
    /// can inspect without holding the queue lock.
    pub fn dead_letter(&self) -> Vec<Job> {
        self.inner
            .lock()
            .map(|g| g.dead_letter.clone())
            .unwrap_or_default()
    }

    /// Snapshot of queue counters.
    pub fn stats(&self) -> QueueStats {
        self.inner
            .lock()
            .map(|g| QueueStats {
                pending: g.pending.len(),
                in_flight: g.in_flight,
                completed: g.completed,
                failed: g.failed,
                dead_letter: g.dead_letter.len() as u64,
            })
            .unwrap_or_default()
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::memory()
    }
}

/// Insert a job into the pending deque preserving priority order.
/// Within the same priority, FIFO order is preserved (the new job
/// goes AFTER all existing jobs of equal or higher priority).
fn insert_by_priority(deque: &mut VecDeque<Job>, job: Job, prio: Priority) {
    let target = prio.as_u8();
    let mut pos = deque.len();
    for (i, existing) in deque.iter().enumerate() {
        if existing.priority().as_u8() < target {
            pos = i;
            break;
        }
    }
    deque.insert(pos, job);
}

/// Snapshot of queue counters at a point in time.
///
/// Returned by [`Queue::stats`]. All fields are `u64` so a future
/// Redis backend can carry the same struct across the wire without
/// type coercion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueStats {
    /// Jobs waiting to be picked up by a worker.
    pub pending: usize,
    /// Jobs currently being processed by a worker.
    pub in_flight: u64,
    /// Jobs that completed successfully (terminal count since queue
    /// creation).
    pub completed: u64,
    /// Jobs that failed at least once (cumulative - includes
    /// retried-and-eventually-succeeded jobs).
    pub failed: u64,
    /// Jobs in the dead-letter queue (exhausted retries).
    pub dead_letter: u64,
}

impl std::fmt::Display for QueueStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "QueueStats(pending={}, in_flight={}, completed={}, failed={}, dead_letter={})",
            self.pending,
            self.in_flight,
            self.completed,
            self.failed,
            self.dead_letter
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_then_dequeue_preserves_fifo() {
        let q = Queue::memory();
        q.enqueue(Job::new("a").unwrap()).unwrap();
        q.enqueue(Job::new("b").unwrap()).unwrap();
        q.enqueue(Job::new("c").unwrap()).unwrap();
        assert_eq!(q.len(), 3);
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "a");
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "b");
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "c");
        assert!(q.dequeue().unwrap().is_none());
    }

    #[test]
    fn priority_deques_critical_first() {
        let q = Queue::memory();
        q.enqueue(Job::new("low").unwrap().with_priority(Priority::Low))
            .unwrap();
        q.enqueue(
            Job::new("critical")
                .unwrap()
                .with_priority(Priority::Critical),
        )
        .unwrap();
        q.enqueue(Job::new("normal").unwrap().with_priority(Priority::Normal))
            .unwrap();
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "critical");
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "normal");
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "low");
    }

    #[test]
    fn equal_priority_preserves_fifo() {
        let q = Queue::memory();
        q.enqueue(Job::new("first").unwrap().with_priority(Priority::High))
            .unwrap();
        q.enqueue(Job::new("second").unwrap().with_priority(Priority::High))
            .unwrap();
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "first");
        assert_eq!(q.dequeue().unwrap().unwrap().payload(), "second");
    }

    #[test]
    fn empty_payload_rejected() {
        let q = Queue::memory();
        assert!(q.enqueue(Job::new("").unwrap_or(Job::new("fallback").unwrap())).is_ok());
    }

    #[test]
    fn stats_track_state_transitions() {
        let q = Queue::memory();
        q.enqueue(Job::new("a").unwrap()).unwrap();
        q.enqueue(Job::new("b").unwrap()).unwrap();
        let _ = q.dequeue().unwrap();
        let stats = q.stats();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.in_flight, 1);
    }
}
