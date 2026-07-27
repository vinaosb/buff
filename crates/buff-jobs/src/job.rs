use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{Backoff, JobsError};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

impl JobId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Priority {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn default_priority() -> Self {
        Self::Normal
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::default_priority()
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    DeadLetter,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::DeadLetter => "dead_letter",
        };
        f.write_str(s)
    }
}

pub type JobResult = Result<(), String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    id: JobId,
    payload: String,
    priority: Priority,
    max_retries: u32,
    backoff: Backoff,
    attempts: u32,
    pub(crate) status: JobStatus,
}

impl Job {
    pub fn new(payload: impl Into<String>) -> Result<Self, JobsError> {
        let payload = payload.into();
        if payload.is_empty() {
            return Err(JobsError::invalid_job("payload must be non-empty"));
        }
        let id = JobId(format!("{}", uuid::Uuid::new_v4()));
        Ok(Self {
            id,
            payload,
            priority: Priority::default(),
            max_retries: 3,
            backoff: Backoff::default(),
            attempts: 0,
            status: JobStatus::Pending,
        })
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn id(&self) -> &JobId {
        &self.id
    }

    pub fn payload(&self) -> &str {
        &self.payload
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    pub fn backoff(&self) -> &Backoff {
        &self.backoff
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn status(&self) -> JobStatus {
        self.status
    }

    pub fn next_retry_delay(&self) -> Result<Option<Duration>, JobsError> {
        let next_attempt = self.attempts.saturating_add(1);
        if next_attempt > self.max_retries {
            return Ok(None);
        }
        let d = self.backoff.delay(next_attempt, self.max_retries)?;
        Ok(Some(d))
    }

    pub(crate) fn mark_in_progress(&mut self) {
        self.status = JobStatus::InProgress;
    }

    pub(crate) fn mark_completed(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.status = JobStatus::Completed;
    }

    pub(crate) fn mark_failed(&mut self) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts > self.max_retries {
            self.status = JobStatus::DeadLetter;
            false
        } else {
            self.status = JobStatus::Pending;
            true
        }
    }

    pub(crate) fn mark_dead_letter(&mut self) {
        self.status = JobStatus::DeadLetter;
    }
}

impl std::fmt::Display for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Job({}, prio={}, status={}, attempts={}/{})",
            self.id, self.priority, self.status, self.attempts, self.max_retries
        )
    }
}
