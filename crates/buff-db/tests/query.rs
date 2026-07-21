use buff_db::{DbParam, JoinKind, Pool, Query};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER NOT NULL
)";

const ORDERS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total REAL NOT NULL
)";

#[tokio::test]
async fn select_insert_round_trip() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(2), DbParam::Text("Alan".into()), DbParam::Int(41)],
    )
    .await
    .unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(3), DbParam::Text("Grace".into()), DbParam::Int(85)],
    )
    .await
    .unwrap();

    let rows = pool.query("SELECT id, name, age FROM users ORDER BY id", &[]).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get("name").and_then(|v| v.as_text()), Some("Ada"));
    assert_eq!(rows[0].get("age").and_then(|v| v.as_int()), Some(36));
    assert_eq!(rows[2].get("name").and_then(|v| v.as_text()), Some("Grace"));
    assert_eq!(rows[2].get("age").and_then(|v| v.as_int()), Some(85));
}

#[tokio::test]
async fn select_update_delete_round_trip() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();

    let n = pool
        .execute(
            "UPDATE users SET age = ? WHERE id = ?",
            &[DbParam::Int(37), DbParam::Int(1)],
        )
        .await
        .unwrap();
    assert_eq!(n, 1);

    let row = pool
        .query_one("SELECT age FROM users WHERE id = ?", &[DbParam::Int(1)])
        .await
        .unwrap();
    assert_eq!(row.get("age").and_then(|v| v.as_int()), Some(37));

    let n = pool.execute("DELETE FROM users WHERE id = ?", &[DbParam::Int(1)]).await.unwrap();
    assert_eq!(n, 1);

    let rows = pool.query("SELECT * FROM users", &[]).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn transaction_commit_persists() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();

    let tx = pool.begin().await.unwrap();
    tx.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let rows = pool.query("SELECT * FROM users", &[]).await.unwrap();
    assert_eq!(rows.len(), 1, "row should be persisted after commit");
}

#[tokio::test]
async fn transaction rollback_does_not_persist() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();

    let tx = pool.begin().await.unwrap();
    tx.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();

    let rows = pool.query("SELECT * FROM users", &[]).await.unwrap();
    assert!(rows.is_empty(), "row should be rolled back");
}

#[tokio::test]
async fn transaction drop_without_commit_rolls_back() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();

    {
        let mut tx = pool.begin().await.unwrap();
        tx.execute(
            "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
            &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
        )
        .await
        .unwrap();
    }

    let rows = pool.query("SELECT * FROM users", &[]).await.unwrap();
    assert!(rows.is_empty(), "tx dropped without commit should auto-rollback");
}

#[tokio::test]
async fn transaction_query_inside_tx() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let rows = tx.query("SELECT * FROM users", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn join_round_trip() {
    let pool = Pool::connect("sqlite::memory:").await.unwrap();
    pool.execute(SCHEMA, &[]).await.unwrap();
    pool.execute(ORDERS_SCHEMA, &[]).await.unwrap();
    pool.execute(
        "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
        &[DbParam::Int(1), DbParam::Text("Ada".into()), DbParam::Int(36)],
    )
    .await
    .unwrap();
    pool.execute(
        "INSERT INTO orders (id, user_id, total) VALUES (?, ?, ?)",
        &[DbParam::Int(100), DbParam::Int(1), DbParam::Float(99.5)],
    )
    .await
    .unwrap();

    let rows = pool
        .query(
            "SELECT u.name, o.total FROM users u INNER JOIN orders o ON o.user_id = u.id",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("name").and_then(|v| v.as_text()), Some("Ada"));
    let total = rows[0].get("total").and_then(|v| v.as_float()).unwrap_or(0.0);
    assert!((total - 99.5).abs() < 1e-6);
}

#[test]
fn query_builder_select_all() {
    assert_eq!(Query::new("users").sql(), "SELECT * FROM users");
}

#[test]
fn query_builder_acceptance_test() {
    let q = Query::new("users")
        .select(&["id", "name"])
        .filter("age > 18");
    assert_eq!(q.sql(), "SELECT id, name FROM users WHERE age > 18");
}

#[test]
fn query_builder_full_pipeline() {
    let q = Query::new("users")
        .select(&["id", "name", "email"])
        .inner_join("profiles", "profiles.user_id = users.id")
        .filter("age > 18")
        .filter("active = true")
        .order_by("name")
        .limit(10);
    assert_eq!(
        q.sql(),
        "SELECT id, name, email FROM users \
         INNER JOIN profiles ON profiles.user_id = users.id \
         WHERE age > 18 AND active = true \
         ORDER BY name \
         LIMIT 10"
    );
}

#[test]
fn query_builder_join_kinds() {
    assert!(Query::new("a").inner_join("b", "b.id = a.id").sql().contains("INNER JOIN"));
    assert!(Query::new("a").left_join("b", "b.id = a.id").sql().contains("LEFT JOIN"));
    let q = Query::new("a").join(JoinKind::Right, "b", "b.id = a.id");
    assert!(q.sql().contains("RIGHT JOIN"));
}

#[test]
fn query_builder_limit_offset() {
    let q = Query::new("users").limit(10).offset(20);
    assert_eq!(q.sql(), "SELECT * FROM users LIMIT 10 OFFSET 20");
}

#[test]
fn query_builder_to_string_via_display() {
    let q = Query::new("users").select(&["id"]);
    assert_eq!(format!("{q}"), "SELECT id FROM users");
}

#[test]
fn row_to_map_conversion() {
    use buff_db::{DbValue, Row};
    let row = Row {
        columns: vec!["id".into(), "name".into(), "active".into()],
        values: vec![DbValue::Int(1), DbValue::Text("Ada".into()), DbValue::Bool(true)],
    };
    let m = row.to_map();
    assert_eq!(m.get("id"), Some(&"1".to_string()));
    assert_eq!(m.get("name"), Some(&"Ada".to_string()));
    assert_eq!(m.get("active"), Some(&"true".to_string()));
}

#[test]
fn db_value_accessors() {
    use buff_db::DbValue;
    assert_eq!(DbValue::Int(42).as_int(), Some(42));
    assert_eq!(DbValue::Int(42).as_float(), Some(42.0));
    assert_eq!(DbValue::Float(1.5).as_float(), Some(1.5));
    assert_eq!(DbValue::Text("hi".into()).as_text(), Some("hi"));
    assert_eq!(DbValue::Bool(true).as_bool(), Some(true));
    assert!(DbValue::Null.is_null());
    assert!(!DbValue::Int(0).is_null());
}

#[test]
fn db_param_from_conversions() {
    use buff_db::DbParam;
    assert!(matches!(DbParam::from(42_i64), DbParam::Int(42)));
    assert!(matches!(DbParam::from(1.5_f64), DbParam::Float(_)));
    assert!(matches!(DbParam::from("hi"), DbParam::Text(_)));
    assert!(matches!(DbParam::from(String::from("x")), DbParam::Text(_)));
    assert!(matches!(DbParam::from(true), DbParam::Bool(true)));
    assert!(matches!(DbParam::from(vec![1_u8, 2]), DbParam::Bytes(_)));
}
