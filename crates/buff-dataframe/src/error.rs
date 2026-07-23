use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum DfError {
    Io(String),
    Csv(String),
    Json(String),
    SchemaMismatch {
        column: String,
        detail: String,
    },
    UnknownColumn(String),
    TypeMismatch {
        column: String,
        expected: &'static str,
    },
    Empty,
}

impl fmt::Display for DfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DfError::Io(s) => write!(f, "io error: {s}"),
            DfError::Csv(s) => write!(f, "csv error: {s}"),
            DfError::Json(s) => write!(f, "json error: {s}"),
            DfError::SchemaMismatch { column, detail } => {
                write!(f, "schema mismatch on column {column:?}: {detail}")
            }
            DfError::UnknownColumn(c) => write!(f, "unknown column: {c:?}"),
            DfError::TypeMismatch { column, expected } => {
                write!(f, "type mismatch on column {column:?}: expected {expected}")
            }
            DfError::Empty => write!(f, "dataframe is empty"),
        }
    }
}

impl std::error::Error for DfError {}

impl From<std::io::Error> for DfError {
    fn from(e: std::io::Error) -> Self {
        DfError::Io(e.to_string())
    }
}

impl From<csv::Error> for DfError {
    fn from(e: csv::Error) -> Self {
        DfError::Csv(e.to_string())
    }
}

impl From<serde_json::Error> for DfError {
    fn from(e: serde_json::Error) -> Self {
        DfError::Json(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, DfError>;
