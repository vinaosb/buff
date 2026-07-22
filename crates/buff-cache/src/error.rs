use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache capacity must be greater than zero (got {requested})")]
    InvalidCapacity { requested: u64 },

    #[error("cache TTL must be non-negative (got {secs}s)")]
    InvalidTtl { secs: u64 },

    #[error("cache key cannot be empty")]
    EmptyKey,

    #[error("internal error: cache operation panicked")]
    Panic,
}
