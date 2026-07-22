pub mod backoff;
pub mod error;
pub mod job;
pub mod queue;
pub mod scheduler;
pub mod worker;

pub use backoff::Backoff;
pub use error::{JobsError, JobsResult};
pub use job::{Job, JobId, JobResult, JobStatus, Priority};
pub use queue::{Queue, QueueStats};
pub use scheduler::{Schedule, Scheduler};
pub use worker::{Worker, WorkerStats};

pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
