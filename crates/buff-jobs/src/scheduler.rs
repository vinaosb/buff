use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::error::{JobsError, JobsResult};
use crate::job::{Job, JobId};

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

#[derive(Debug, Clone)]
pub struct ScheduledJob {
    pub id: JobId,
    pub schedule: Schedule,
    pub job: Job,
    pub next_fire: Option<Instant>,
}

pub struct Scheduler {
    scheduled: Arc<Mutex<Vec<ScheduledJob>>>,
    running: Arc<tokio::sync::atomic::AtomicBool>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Scheduler {
    pub fn new() -> Self {
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        Scheduler {
            scheduled: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(tokio::sync::atomic::AtomicBool::new(false)),
            shutdown: shutdown_tx,
        }
    }

    pub async fn cron(&self, expr: impl Into<String>, job: Job) -> JobsResult<JobId> {
        let expr = expr.into();
        let _schedule = cron::Schedule::from_str(&expr)
            .map_err(|e| JobsError::invalid_cron(&expr, e.to_string()))?;

        let id = job.id().clone();
        let mut scheduled = self.scheduled.lock().await;
        scheduled.push(ScheduledJob {
            id,
            schedule: Schedule::Cron(expr),
            job,
            next_fire: None,
        });
        Ok(id)
    }

    pub async fn interval(&self, duration: Duration, job: Job) -> JobsResult<JobId> {
        let id = job.id().clone();
        let mut scheduled = self.scheduled.lock().await;
        scheduled.push(ScheduledJob {
            id,
            schedule: Schedule::Interval(duration),
            job,
            next_fire: Some(Instant::now() + duration),
        });
        Ok(id)
    }

    pub async fn start(&self) {
        use tokio::sync::atomic::Ordering;
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
                        let mut sched = scheduled.lock().await;
                        for sj in sched.iter_mut() {
                            if let Some(next) = sj.next_fire {
                                if next <= now {
                                    match &sj.schedule {
                                        Schedule::Interval(d) => {
                                            sj.next_fire = Some(now + *d);
                                        }
                                        Schedule::Cron(expr) => {
                                            if let Ok(schedule) = cron::Schedule::from_str(expr) {
                                                let upcoming: Vec<_> = schedule.upcoming(chrono::Utc).take(1).collect();
                                                if let Some(next_time) = upcoming.first() {
                                                    let duration_until = (*next_time - chrono::Utc::now())
                                                        .to_std()
                                                        .unwrap_or(Duration::from_secs(60));
                                                    sj.next_fire = Some(now + duration_until);
                                                }
                                            }
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
        use tokio::sync::atomic::Ordering;
        self.running.store(false, Ordering::SeqCst);
        let _ = self.shutdown.send(true);
    }

    pub fn is_running(&self) -> bool {
        use tokio::sync::atomic::Ordering;
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
