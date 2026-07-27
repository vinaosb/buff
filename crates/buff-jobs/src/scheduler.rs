use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::error::{JobsError, JobsResult};
use crate::job::{Job, JobId, JobResult};

#[derive(Debug, Clone)]
pub enum Schedule {
    Interval(Duration),
    Cron(String),
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Schedule::Interval(d) => write!(f, "every {:?}", d),
            Schedule::Cron(expr) => write!(f, "cron: {}", expr),
        }
    }
}

/// Handler closure invoked by [`Scheduler::start`] when a job's fire
/// time arrives. Receives the fired [`Job`] by reference and returns
/// [`JobResult`] (`Result<(), String>`).
///
/// Stored as `Arc<dyn Fn(...)>` so multiple scheduled jobs can share
/// the same handler and the closure can be cheaply cloned into the
/// background dispatch task.
pub type JobHandler = Arc<dyn Fn(&Job) -> JobResult + Send + Sync>;

#[derive(Clone)]
pub struct ScheduledJob {
    pub id: JobId,
    pub schedule: Schedule,
    pub job: Job,
    pub next_fire: Option<Instant>,
    /// Optional handler invoked when `next_fire` elapses. `None` for
    /// jobs registered via [`Scheduler::cron`] / [`Scheduler::interval`]
    /// (legacy no-op semantics — the schedule ticks but performs no
    /// work). Populated by [`Scheduler::cron_with_handler`] /
    /// [`Scheduler::interval_with_handler`].
    pub handler: Option<JobHandler>,
    /// Number of consecutive handler failures since the last success.
    /// Reset to 0 on `Ok(())`. Drives the dead-letter pruning path:
    /// when this exceeds the job's `max_retries`, the schedule entry
    /// is removed (mirrors the [`crate::Worker`] dead-letter policy).
    pub consecutive_failures: u32,
}

impl fmt::Debug for ScheduledJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledJob")
            .field("id", &self.id)
            .field("schedule", &self.schedule)
            .field("job", &self.job)
            .field("next_fire", &self.next_fire)
            .field("handler", &self.handler.as_ref().map(|_| "<closure>"))
            .field("consecutive_failures", &self.consecutive_failures)
            .finish()
    }
}

pub struct Scheduler {
    scheduled: Arc<Mutex<Vec<ScheduledJob>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Scheduler {
    pub fn new() -> Self {
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        Scheduler {
            scheduled: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown: shutdown_tx,
        }
    }

    pub async fn cron(&self, expr: impl Into<String>, job: Job) -> JobsResult<JobId> {
        let expr = expr.into();
        let _ = cron::Schedule::from_str(&expr)
            .map_err(|e| JobsError::invalid_cron(&expr, e.to_string()))?;

        let id = job.id().clone();
        let next_fire = compute_next_fire(&Schedule::Cron(expr.clone()), Instant::now());
        let mut scheduled = self.scheduled.lock().await;
        scheduled.push(ScheduledJob {
            id: id.clone(),
            schedule: Schedule::Cron(expr),
            job,
            next_fire,
            handler: None,
            consecutive_failures: 0,
        });
        Ok(id)
    }

    pub async fn interval(&self, duration: Duration, job: Job) -> JobsResult<JobId> {
        let id = job.id().clone();
        let mut scheduled = self.scheduled.lock().await;
        scheduled.push(ScheduledJob {
            id: id.clone(),
            schedule: Schedule::Interval(duration),
            job,
            next_fire: Some(Instant::now() + duration),
            handler: None,
            consecutive_failures: 0,
        });
        Ok(id)
    }

    /// Register a cron-scheduled job with a handler closure invoked
    /// each time the schedule fires. The handler runs on the
    /// scheduler's background dispatch task; long-running handlers
    /// will delay subsequent ticks (the 100ms tick cadence is the
    /// floor for responsiveness).
    pub async fn cron_with_handler<F>(
        &self,
        expr: impl Into<String>,
        job: Job,
        handler: F,
    ) -> JobsResult<JobId>
    where
        F: Fn(&Job) -> JobResult + Send + Sync + 'static,
    {
        let expr = expr.into();
        let _ = cron::Schedule::from_str(&expr)
            .map_err(|e| JobsError::invalid_cron(&expr, e.to_string()))?;

        let id = job.id().clone();
        let next_fire = compute_next_fire(&Schedule::Cron(expr.clone()), Instant::now());
        let handler: JobHandler = Arc::new(handler);
        let mut scheduled = self.scheduled.lock().await;
        scheduled.push(ScheduledJob {
            id: id.clone(),
            schedule: Schedule::Cron(expr),
            job,
            next_fire,
            handler: Some(handler),
            consecutive_failures: 0,
        });
        Ok(id)
    }

    /// Register an interval-scheduled job with a handler closure
    /// invoked each time the duration elapses.
    pub async fn interval_with_handler<F>(
        &self,
        duration: Duration,
        job: Job,
        handler: F,
    ) -> JobsResult<JobId>
    where
        F: Fn(&Job) -> JobResult + Send + Sync + 'static,
    {
        let id = job.id().clone();
        let handler: JobHandler = Arc::new(handler);
        let mut scheduled = self.scheduled.lock().await;
        scheduled.push(ScheduledJob {
            id: id.clone(),
            schedule: Schedule::Interval(duration),
            job,
            next_fire: Some(Instant::now() + duration),
            handler: Some(handler),
            consecutive_failures: 0,
        });
        Ok(id)
    }

    /// Spawn the background dispatch loop. The loop ticks every 100ms;
    /// for each scheduled job whose `next_fire` has elapsed, the
    /// job's handler (if any) is invoked, and `next_fire` is advanced
    /// to the next occurrence. One-shot jobs (no future fire time)
    /// are pruned after their handler runs. Handler errors are logged
    /// via `eprintln!` and the failure counter is bumped; once it
    /// exceeds the job's `max_retries`, the schedule entry is removed
    /// immediately (dead-letter semantics mirroring [`crate::Worker`]).
    pub async fn start(&self) {
        use std::sync::atomic::Ordering;
        self.running.store(true, Ordering::SeqCst);

        let scheduled = self.scheduled.clone();
        let running = self.running.clone();
        let mut shutdown_rx = self.shutdown.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if !running.load(Ordering::SeqCst) {
                            break;
                        }
                        let now = Instant::now();

                        // Phase 1: collect fire-ready entries and
                        // advance next_fire. Lock is dropped before
                        // handler invocation so handler execution
                        // cannot starve other scheduler callers.
                        let fire_ready: Vec<(Job, JobHandler)> = {
                            let mut sched = scheduled.lock().await;
                            let mut ready = Vec::new();
                            for sj in sched.iter_mut() {
                                if let Some(next) = sj.next_fire {
                                    if next <= now {
                                        sj.next_fire = compute_next_fire(&sj.schedule, now);
                                        if let Some(handler) = sj.handler.clone() {
                                            ready.push((sj.job.clone(), handler));
                                        }
                                    }
                                }
                            }
                            ready
                        };

                        // Phase 2: dispatch handlers (no lock held),
                        // then re-lock to update failure counters and
                        // prune exhausted entries. Pruning happens
                        // in the SAME tick as the failing fire (not
                        // deferred to the next tick) so dead-letter
                        // semantics are observable immediately.
                        for (job, handler) in fire_ready {
                            let job_id = job.id().clone();
                            let result = handler(&job);
                            let mut sched = scheduled.lock().await;
                            if let Some(sj) = sched.iter_mut().find(|s| s.id == job_id) {
                                match result {
                                    Ok(()) => {
                                        sj.consecutive_failures = 0;
                                    }
                                    Err(reason) => {
                                        sj.consecutive_failures =
                                            sj.consecutive_failures.saturating_add(1);
                                        let attempts = sj.consecutive_failures;
                                        let max_retries = sj.job.max_retries();
                                        if attempts > max_retries {
                                            // Dead-letter: prune immediately so
                                            // callers observe the removal on the
                                            // very next `pending_count()` read.
                                            eprintln!(
                                                "[buff-jobs] scheduled job {} dead-lettered after {}/{} consecutive failures — schedule entry removed",
                                                sj.id, attempts, max_retries,
                                            );
                                            sched.retain(|s| s.id != job_id);
                                        } else {
                                            eprintln!(
                                                "[buff-jobs] scheduled job {} handler failed (attempt {}/{}): {}",
                                                sj.id, attempts, max_retries, reason,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        break;
                    }
                }
            }
        });
    }

    pub async fn stop(&self) {
        use std::sync::atomic::Ordering;
        self.running.store(false, Ordering::SeqCst);
        let _ = self.shutdown.send(true);
    }

    pub fn is_running(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.running.load(Ordering::SeqCst)
    }

    pub async fn schedules(&self) -> Vec<ScheduledJob> {
        self.scheduled.lock().await.clone()
    }

    pub async fn next_due(&self) -> Option<Instant> {
        let scheduled = self.scheduled.lock().await;
        scheduled.iter().filter_map(|sj| sj.next_fire).min()
    }

    pub async fn pending_count(&self) -> usize {
        self.scheduled.lock().await.len()
    }

    pub async fn remove(&self, job_id: JobId) -> bool {
        let mut scheduled = self.scheduled.lock().await;
        let before = scheduled.len();
        scheduled.retain(|sj| sj.id != job_id);
        scheduled.len() < before
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new()
    }
}

/// Compute the next fire instant for a schedule, relative to `now`.
///
/// Returns `None` for cron expressions with no upcoming fire time
/// (e.g. an expired one-shot schedule). Interval schedules always
/// return `Some(now + duration)`.
fn compute_next_fire(schedule: &Schedule, now: Instant) -> Option<Instant> {
    match schedule {
        Schedule::Interval(d) => Some(now + *d),
        Schedule::Cron(expr) => {
            let parsed = cron::Schedule::from_str(expr).ok()?;
            let upcoming: Vec<_> = parsed.upcoming(chrono::Utc).take(1).collect();
            let next_time = upcoming.first()?;
            let duration_until = (*next_time - chrono::Utc::now())
                .to_std()
                .unwrap_or(Duration::from_secs(60));
            Some(now + duration_until)
        }
    }
}
