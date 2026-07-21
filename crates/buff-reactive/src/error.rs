use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReactiveError {
    #[error("reactive borrow conflict: {detail}")]
    BorrowConflict { detail: String },
    #[error("reactive closure panicked: {detail}")]
    ClosurePanic { detail: String },
}

pub type Result<T> = std::result::Result<T, ReactiveError>;
