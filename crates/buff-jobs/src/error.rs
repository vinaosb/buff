use thiserror::Error;

pub type JobsResult<T> = Result<T, JobsError>;

#[derive(Debug, Error)]
pub enum JobsError {
    #[error("invalid cron expression `{expr}`: {reason}")]
    InvalidCron { expr: String, reason: String },

    #[error("invalid job id `{0}`")]
    InvalidJobId(String),

    #[error("invalid job: {0}")]
    InvalidJob(String),

    #[error("invalid backoff: {0}")]
    InvalidBackoff(String),

    #[error("invalid retry attempt {attempt} (max_retries={max_retries})")]
    InvalidAttempt { attempt: u32, max_retries: u32 },

    #[error("job `{job_id}` permanently failed: {reason}")]
    PermanentFailure { job_id: String, reason: String },

    #[error("internal error: jobs operation panicked")]
    Panic,
}

impl JobsError {
    pub(crate) fn invalid_cron(expr: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidCron {
            expr: expr.into(),
            reason: reason.into(),
        }
    }

    pub(crate) fn invalid_job(reason: impl Into<String>) -> Self {
        Self::InvalidJob(reason.into())
    }

    pub(crate) fn invalid_backoff(reason: impl Into<String>) -> Self {
        Self::InvalidBackoff(reason.into())
    }
}

impl From<cron::error::Error> for JobsError {
    fn from(err: cron::error::Error) -> Self {
        JobsError::InvalidCron {
            expr: String::new(),
            reason: err.to_string(),
        }
    }
}
