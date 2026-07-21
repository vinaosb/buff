use sqlx::any::{AnyPool, AnyPoolOptions};

use crate::error::{DbError, Result};
use crate::row::{row_from_any, Row};

/// Database connection pool — the runtime-value type backing Buff's
/// prelude `Database` namespace. Wraps `sqlx::any::AnyPool` so the same
/// surface serves both SQLite and PostgreSQL (and any future `sqlx`
/// driver added to the cargo features list).
///
/// Construct via [`Pool::connect`] (the codegen lowering of Buff's
/// `Database.connect(url)` assoc fn). Then drive SQL via [`query`](Self::query)
/// / [`execute`](Self::execute) / [`begin`](Self::begin).
///
/// Send + 'static — safe to capture in `spawn` closures (per FFI guide R4).
#[derive(Debug, Clone)]
pub struct Pool {
    inner: AnyPool,
}

impl Pool {
    pub async fn connect(url: &str) -> Result<Pool> {
        validate_driver(url)?;
        let pool = AnyPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await
            .map_err(DbError::from)?;
        Ok(Pool { inner: pool })
    }

    pub fn from_inner(inner: AnyPool) -> Pool {
        Pool { inner }
    }

    pub fn inner(&self) -> &AnyPool {
        &self.inner
    }

    pub async fn query(&self, sql: &str, params: &[DbParam]) -> Result<Vec<Row>> {
        let mut q = sqlx::query(sql);
        for p in params {
            q = p.bind_to(q);
        }
        let rows = q.fetch_all(&self.inner).await.map_err(DbError::from)?;
        rows.iter().map(row_from_any).collect()
    }

    pub async fn query_one(&self, sql: &str, params: &[DbParam]) -> Result<Row> {
        let mut q = sqlx::query(sql);
        for p in params {
            q = p.bind_to(q);
        }
        let row = q.fetch_optional(&self.inner).await.map_err(DbError::from)?;
        match row {
            Some(r) => row_from_any(&r),
            None => Err(DbError::Query("no rows returned".into())),
        }
    }

    pub async fn execute(&self, sql: &str, params: &[DbParam]) -> Result<u64> {
        let mut q = sqlx::query(sql);
        for p in params {
            q = p.bind_to(q);
        }
        let res = q.execute(&self.inner).await.map_err(DbError::from)?;
        Ok(res.rows_affected())
    }

    pub async fn begin(&self) -> Result<Transaction> {
        let tx = self.inner.begin().await.map_err(DbError::from)?;
        Ok(Transaction { inner: Some(tx) })
    }

    pub fn url_scheme(&self) -> &str {
        self.inner.any_kind().name()
    }
}

fn validate_driver(url: &str) -> Result<()> {
    let scheme = url.split(':').next().unwrap_or("");
    match scheme {
        "sqlite" | "postgres" | "postgresql" => Ok(()),
        "" => Err(DbError::InvalidUrl("(empty)".into())),
        other => Err(DbError::UnsupportedDriver(other.to_string())),
    }
}

#[derive(Debug, Clone)]
pub struct Transaction<'a> {
    inner: Option<sqlx::Transaction<'a, sqlx::Any>>,
}

impl<'a> Transaction<'a> {
    pub async fn commit(mut self) -> Result<()> {
        match self.inner.take() {
            Some(tx) => tx.commit().await.map_err(DbError::from),
            None => Err(DbError::Transaction("already completed".into())),
        }
    }

    pub async fn rollback(mut self) -> Result<()> {
        match self.inner.take() {
            Some(tx) => tx.rollback().await.map_err(DbError::from),
            None => Err(DbError::Transaction("already completed".into())),
        }
    }

    pub async fn execute(&mut self, sql: &str, params: &[DbParam]) -> Result<u64> {
        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| DbError::Transaction("already completed".into()))?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = p.bind_to(q);
        }
        let res = q.execute(&mut **tx).await.map_err(DbError::from)?;
        Ok(res.rows_affected())
    }

    pub async fn query(&mut self, sql: &str, params: &[DbParam]) -> Result<Vec<Row>> {
        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| DbError::Transaction("already completed".into()))?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = p.bind_to(q);
        }
        let rows = q.fetch_all(&mut **tx).await.map_err(DbError::from)?;
        rows.iter().map(row_from_any).collect()
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        if let Some(tx) = self.inner.take() {
            drop(tx);
        }
    }
}

#[derive(Debug, Clone)]
pub enum DbParam {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Bytes(Vec<u8>),
}

impl DbParam {
    pub fn bind_to<'q>(
        &'q self,
        q: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    ) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
        match self {
            DbParam::Null => q.bind(None::<i64>),
            DbParam::Int(n) => q.bind(n),
            DbParam::Float(n) => q.bind(n),
            DbParam::Text(s) => q.bind(s.as_str()),
            DbParam::Bool(b) => q.bind(b),
            DbParam::Bytes(b) => q.bind(b.as_slice()),
        }
    }
}

impl From<i64> for DbParam {
    fn from(n: i64) -> Self {
        DbParam::Int(n)
    }
}

impl From<f64> for DbParam {
    fn from(n: f64) -> Self {
        DbParam::Float(n)
    }
}

impl From<&str> for DbParam {
    fn from(s: &str) -> Self {
        DbParam::Text(s.to_string())
    }
}

impl From<String> for DbParam {
    fn from(s: String) -> Self {
        DbParam::Text(s)
    }
}

impl From<bool> for DbParam {
    fn from(b: bool) -> Self {
        DbParam::Bool(b)
    }
}

impl From<Vec<u8>> for DbParam {
    fn from(b: Vec<u8>) -> Self {
        DbParam::Bytes(b)
    }
}
