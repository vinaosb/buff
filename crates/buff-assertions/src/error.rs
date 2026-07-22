use std::fmt;

#[derive(Debug, Clone)]
pub struct AssertionError {
    pub message: String,
}

impl AssertionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AssertionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "assertion failed: {}", self.message)
    }
}

impl std::error::Error for AssertionError {}
