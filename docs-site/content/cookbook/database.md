+++
title = "Database"
weight = 44
+++

# Database recipes

Recipes for SQL access via the `Database` prelude type (T18, v1.15
frameworks wave 3). `Database` wraps the pure-Rust `sqlx` crate
(SQLite + PostgreSQL via `runtime-tokio-rustls` — no native-tls, no
libpq, no cc-rs).

> **Status:** `Database` carries an EXPERIMENTAL stability badge
> through v1.18. `Database.connect(url)` ships today; the `Pool`
> instance-method surface (`query`, `execute`, `begin`) lands with
> the coordinated `Type::Pool` sibling task.

## Connect to a database

**Problem**: Open a connection pool to a SQLite or Postgres database.

**Solution**:

```buff
func open_db(url: String) -> Result<Pool, Error>:
    return Database.connect(url)

func main():
    match open_db("sqlite://app.db") {
        Ok(pool) => print("connected"),
        Err(e)   => print("connect failed: " + e.string()),
    }
```

**Explanation**:

`Database.connect(url)` is the prelude surface for
`sqlx::any::AnyPool::connect(url).await` (T18). It returns a `Pool`
value (clone-cheap — `sqlx::AnyPool` is internally `Arc`-shared) or
an `Error` on failure. The URL scheme selects the driver:
`sqlite://`, `postgres://`, or `postgresql://`.

`buff-db` records `sqlx` + `tokio` in codegen `extern_crates` when
a Buff program uses `Database.*`. The async runtime is auto-injected
by the compiler (`Pool.connect` is async under the hood); Buff has no
`await` keyword, so the call site reads as synchronous.

## Run a SELECT query

**Problem**: Read rows from a table.

**Solution**:

```buff
func list_users(pool: Pool) -> Result<Vector<Row>, Error>:
    return pool.query("SELECT id, name FROM users", [])

func main():
    let pool = Database.connect("sqlite://app.db")?
    match list_users(pool) {
        Ok(rows) =>
            for row in rows:
                print(row.get("name"))
        Err(e) => print("query failed: " + e.string()),
    }
```

**Explanation**:

`pool.query(sql, params)` runs the SQL string with the bound params
vector and returns a `Vector<Row>`. Each `Row` exposes `.get(col)`
returning `Option<String>` (None when the column is NULL or absent —
never panics). For typed access, `.get_int(col)`, `.get_float(col)`,
`.get_bool(col)` accessors are planned.

The second argument (`[]`) is the bind parameters — empty for a
static query. Always bind user-supplied values; never splice them
into the SQL string:

```buff
// GOOD — bound param
pool.query("SELECT * FROM users WHERE id = ?", [user_id])

// BAD — SQL injection
pool.query("SELECT * FROM users WHERE id = " + user_id, [])
```

`sqlx` escapes bound params correctly at the protocol layer.

## Insert a row

**Problem**: Add a new row and learn how many rows were affected.

**Solution**:

```buff
func create_user(pool: Pool, name: String, email: String) -> Result<Int, Error>:
    let affected = pool.execute(
        "INSERT INTO users (name, email) VALUES (?, ?)",
        [name, email]
    )?
    return Ok(affected)

func main():
    let pool = Database.connect("sqlite://app.db")?
    match create_user(pool, "Ada", "ada@example.com") {
        Ok(n)  => print("inserted " + n.string() + " row(s)"),
        Err(e) => print("insert failed: " + e.string()),
    }
```

**Explanation**:

`pool.execute(sql, params)` runs a statement that doesn't return rows
(INSERT/UPDATE/DELETE) and returns the affected-row count as `Int`.
For INSERT-with-returning (Postgres `RETURNING *`), use `pool.query`
instead — `sqlx` will run the query and surface the returned rows.

Params are positional (`?` placeholders). Named params (`:name`)
are on the v1.18+ roadmap; for now, position them carefully or build
the params vector explicitly with comments.

## Wrap statements in a transaction

**Problem**: Run several writes atomically — all-or-nothing rollback.

**Solution**:

```buff
func transfer_funds(pool: Pool, from_id: Int, to_id: Int, amount: Int) -> Result<Void, Error>:
    let tx = pool.begin()?
    tx.execute("UPDATE accounts SET balance = balance - ? WHERE id = ?", [amount, from_id])?
    tx.execute("UPDATE accounts SET balance = balance + ? WHERE id = ?", [amount, to_id])?
    tx.commit()?
    return Ok(())

func main():
    let pool = Database.connect("sqlite://bank.db")?
    match transfer_funds(pool, 1, 2, 100) {
        Ok(_)  => print("transferred"),
        Err(e) => print("rolled back: " + e.string()),
    }
```

**Explanation**:

`pool.begin()` returns a `Transaction` (`sqlx::Transaction`). Every
`.execute` / `.query` call on the transaction runs in the same DB
transaction; if any fails, the `?` propagates the error and drops the
`Transaction`, which triggers automatic rollback. `tx.commit()` makes
the changes durable — also fallible, hence the `?`.

The pattern is identical for nested savepoints: `tx.begin()` on a
`Transaction` returns a savepoint-scoped sub-transaction. Rollback of
the sub-transaction doesn't affect the outer one — useful for partial
retries inside a larger unit of work.

## Use a connection pool

**Problem**: Share a single pool across many handler functions (typical
web-server shape).

**Solution**:

```buff
func find_user(pool: Pool, id: Int) -> Result<String, Error>:
    let rows = pool.query("SELECT name FROM users WHERE id = ?", [id.string()])?
    if rows.len() == 0:
        return Error("not found")
    return Ok(rows[0].get("name"))

func count_users(pool: Pool) -> Result<Int, Error>:
    let rows = pool.query("SELECT COUNT(*) AS n FROM users", [])?
    return Ok(Int(rows[0].get("n")))

func main():
    let pool = Database.connect("sqlite://app.db")?
    print(count_users(pool))
    print(find_user(pool, 1))
```

**Explanation**:

`Pool` is `Clone` — cloning it just bumps an internal `Arc` refcount.
Pass clones into handler functions freely; the underlying connection
count stays bounded by `sqlx`'s pool configuration (default 10
connections, override via the URL query string: `sqlite://app.db?
max_connections=50`).

For long-running servers, construct the pool once at startup
(`main`) and thread it through every handler. The `buff-web`
`Request` type doesn't carry pool state today; the recommended
pattern is to close over the pool in the route handler's closure:

```buff
let pool = Database.connect(url)?
app.get(path: "/users/{id}", handler: { req =>
    let id = Int(req.path())
    match find_user(pool, id) {
        Some(name) => Response.text(name),
        None       => Response.status_only(404),
    }
})
```
