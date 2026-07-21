use buff_db::{DbError, DbParam, Pool};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
)";

#[tokio::test]
async fn pool_connect_sqlite_memory() {
    let pool = Pool::connect("sqlite::memory:").await;
    assert!(pool.is_ok(), "sqlite::memory: should connect");
    let pool = pool.unwrap();
    assert_eq!(pool.url_scheme(), "SQLite");
}

#[tokio::test]
async fn pool_connect_unsupported_driver_errors() {
    let err = Pool::connect("mysql://localhost").await;
    assert!(err.is_err());
    let e = err.unwrap_err();
    assert!(matches!(e, DbError::UnsupportedDriver(_)));
    assert!(format!("{e}").contains("mysql"));
}

#[tokio::test]
async fn pool_connect_empty_url_errors() {
    let err = Pool::connect("").await;
    assert!(err.is_err());
    let e = err.unwrap_err();
    assert!(matches!(e, DbError::InvalidUrl(_)));
}

#[tokio::test]
async fn pool_execute_ddl_returns_zero_rows() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    let n = pool.execute(SCHEMA, &[]).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn pool_execute_dml_returns_one_row() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    let n = pool
        .execute(
            "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
            &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
        )
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn pool_query_one_returns_row() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();

    let row = pool
        .query_one("SELECT id, name FROM users WHERE id = ?", &[DbParam::Int(1)])
        .await
        .unwrap();

    assert_eq!(row.get("id").and_then(|v| v.as_int()), Some(1));
    assert_eq!(row.get("name").and_then(|v| v.as_text()), Some("Ada"));
}

#[tokio::test]
async fn pool_query_one_returns_error_when_empty() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    let res = pool.query_one("SELECT * FROM users WHERE id = ?", &[DbParam::Int(999)]).await;
    assert!(res.is_err());
    let e = res.unwrap_err();
    assert!(matches!(e, DbError::Query(_)));
}

#[tokio::test]
async fn pool_clone_is_shareable_across_tasks() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    let p1 = pool.clone();
    let p2 = pool.clone();
    p1.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();
    let rows = p2.query("SELECT * FROM users", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn pool_query_invalid_sql_errors() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    let res = pool.query("SELECT FROM WHERE", &[]).await;
    assert!(res.is_err());
}
