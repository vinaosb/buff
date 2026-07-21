//! Error type for `.buffhtml` codegen.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuffHtmlCodegenError {
    #[error("buffhtml codegen error: {message}")]
    UnsupportedConstruct { message: String },
}
