use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum DbError {
    Pool(String),
    Query(String),
    Execute(String),
    Transaction(String),
    InvalidUrl(String),
    UnsupportedDriver(String),
    ColumnMissing(String),
    Bind(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Pool(s) => write!(f, "pool error: {s}"),
            DbError::Query(s) => write!(f, "query error: {s}"),
            DbError::Execute(s) => write!(f, "execute error: {s}"),
            DbError::Transaction(s) => write!(f, "transaction error: {s}"),
            DbError::InvalidUrl(s) => write!(f, "invalid url: {s}"),
            DbError::UnsupportedDriver(s) => write!(f, "unsupported driver: {s}"),
            DbError::ColumnMissing(c) => write!(f, "column missing: {c:?}"),
            DbError::Bind(s) => write!(f, "bind error: {s}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolClosed | sqlx::Error::PoolTimedOut => DbError::Pool(e.to_string()),
            sqlx::Error::Database(_) => DbError::Query(e.to_string()),
            _ => DbError::Query(e.to_string()),
        }
    }
}

// NOTE: `From<sqlx::migrate::MigrateError>` was removed because the `migrate`
// module is gated behind sqlx's `migrate` cargo feature, which the workspace
// pin does not enable (migrations are deferred per the T18 spec). The crate
// body never invokes migrate, so this impl was unused.

pub type Result<T> = std::result::Result<T, DbError>;
