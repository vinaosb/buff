// T18 example: connect to in-memory SQLite, run a query, print rows.
// The matching simple_query.buff mirrors this pipeline using the
// Buff language surface (`Database.connect` assoc fn).
use buff_db::{DbParam, Pool};

#[tokio::main]
async fn main() {
    let pool = match Pool::connect("sqlite::memory:").await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("connect failed: {e}");
            return;
        }
    };
    println!("connected to {}", pool.url_scheme());

    if let Err(e) = pool
        .execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            &[],
        )
        .await
    {
        eprintln!("create failed: {e}");
        return;
    }

    if let Err(e) = pool
        .execute(
            "INSERT INTO users (id, name) VALUES (?, ?)",
            &[DbParam::Int(1), DbParam::Text("Ada".into())],
        )
        .await
    {
        eprintln!("insert failed: {e}");
        return;
    }

    match pool.query("SELECT id, name FROM users", &[]).await {
        Ok(rows) => {
            for row in &rows {
                let id = row.get("id").and_then(|v| v.as_int()).unwrap_or_default();
                let name = row
                    .get("name")
                    .and_then(|v| v.as_text())
                    .unwrap_or("(null)");
                println!("{id}: {name}");
            }
        }
        Err(e) => eprintln!("query failed: {e}"),
    }
}
