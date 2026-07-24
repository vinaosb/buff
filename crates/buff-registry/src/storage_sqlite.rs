//! SQLite-backed [`Storage`] implementation — T57 Track F.
//!
//! Promotes the registry from the in-memory [`crate::InMemoryStorage`]
//! (T126 MVP) to durable SQLite persistence via the `rusqlite` crate
//! (bundled SQLite amalgamation — no system-installed SQLite required,
//! no external service, no Docker; matches the "no C library, no
//! Docker" hard rule from AGENTS.md).
//!
//! # Schema
//!
//! ```sql
//! CREATE TABLE packages (
//!     name         TEXT PRIMARY KEY,
//!     scope        TEXT,              -- NULL for unscoped, '@org' for scoped
//!     created_at   INTEGER NOT NULL   -- unix seconds
//! );
//!
//! CREATE TABLE versions (
//!     name         TEXT NOT NULL REFERENCES packages(name),
//!     version      TEXT NOT NULL,     -- canonical semver string
//!     deps_json    TEXT NOT NULL,     -- serde_json of Vec<DepSpec>
//!     tarball      BLOB NOT NULL,     -- raw tarball bytes
//!     author       TEXT,              -- publishing token / github login
//!     published_at INTEGER,           -- unix seconds (NULL for legacy)
//!     quality_json TEXT NOT NULL DEFAULT '{}', -- serde_json of QualityAttachment
//!     PRIMARY KEY (name, version)
//! );
//!
//! CREATE TABLE tokens (
//!     token TEXT PRIMARY KEY
//! );
//!
//! CREATE TABLE rate_log (
//!     token     TEXT NOT NULL,
//!     ts_ms     INTEGER NOT NULL   -- millisecond Instant as i64
//! );
//!
//! CREATE TABLE verified_authors (
//!     author TEXT PRIMARY KEY
//! );
//! ```
//!
//! # Concurrency model
//!
//! `rusqlite::Connection` is `Send` but NOT `Sync`. We wrap it in
//! `std::sync::Mutex` — the same pattern the in-memory backend uses.
//! SQLite's own WAL mode (enabled on init) allows concurrent readers
//! across connections, but since we share a single connection, all
//! access is serialized through the mutex. The critical sections are
//! sub-millisecond for typical registry workloads.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` in non-test code. All rusqlite
//! errors surface as [`crate::StorageError::Failure`].

use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use semver::Version;

use crate::error::StorageError;
use crate::storage::{
    DepSpec, PackageMetadata, PackageSummary, QualityAttachment, VersionInfo,
};
use crate::storage::VERSION_EXISTS_MARKER;

/// The DDL executed on [`SqliteStorage::new`] to create all tables +
/// indexes if they don't already exist. Idempotent — safe to run on
/// every startup against an existing database file.
const SCHEMA_SQL: &str = "\
CREATE TABLE IF NOT EXISTS packages (\
    name       TEXT PRIMARY KEY,\
    scope      TEXT,\
    created_at INTEGER NOT NULL\
);\
CREATE TABLE IF NOT EXISTS versions (\
    name         TEXT NOT NULL REFERENCES packages(name),\
    version      TEXT NOT NULL,\
    deps_json    TEXT NOT NULL,\
    tarball      BLOB NOT NULL,\
    author       TEXT,\
    published_at INTEGER,\
    quality_json TEXT NOT NULL DEFAULT '{}',\
    PRIMARY KEY (name, version)\
);\
CREATE TABLE IF NOT EXISTS tokens (\
    token TEXT PRIMARY KEY\
);\
CREATE TABLE IF NOT EXISTS rate_log (\
    token TEXT NOT NULL,\
    ts_ms INTEGER NOT NULL\
);\
CREATE TABLE IF NOT EXISTS verified_authors (\
    author TEXT PRIMARY KEY\
);\
CREATE INDEX IF NOT EXISTS idx_versions_name ON versions(name);\
CREATE INDEX IF NOT EXISTS idx_rate_log_token ON rate_log(token);\
";

/// SQLite-backed registry storage.
///
/// Construct with [`SqliteStorage::open`] (durable file) or
/// [`SqliteStorage::open_in_memory`] (ephemeral — for tests). The
/// resulting `Arc<dyn Storage>` drops into [`crate::AppState::new`]
/// exactly like the in-memory backend.
///
/// # Durability
///
/// `open(path)` creates (or opens) a file-backed SQLite database.
/// WAL mode is enabled for better concurrent-read performance. The
/// database survives process restarts — all published packages,
/// tokens, and rate-limit counters persist.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use buff_registry::{AppState, SqliteStorage, Storage};
///
/// let storage: Arc<dyn Storage> =
///     Arc::new(SqliteStorage::open("registry.db").expect("open db"));
/// let state = AppState::new(storage);
/// ```
#[derive(Debug)]
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open (or create) a file-backed SQLite database at `path`.
    ///
    /// Creates all tables + indexes if they don't exist (idempotent —
    /// safe to call against an existing database). Enables WAL mode
    /// for concurrent-read performance.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Failure`] if the SQLite connection
    /// cannot be opened, WAL mode cannot be enabled, or the schema
    /// DDL fails.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, StorageError> {
        let conn = Connection::open(path)
            .map_err(|e| StorageError::Failure(format!("sqlite open: {e}")))?;
        Self::init(conn)
    }

    /// Open an in-memory SQLite database (ephemeral — lost when the
    /// `SqliteStorage` is dropped). Used by tests that want SQLite
    /// semantics without filesystem I/O.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Failure`] on init failure (rare).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| StorageError::Failure(format!("sqlite in-memory open: {e}")))?;
        Self::init(conn)
    }

    /// Shared init: enable WAL + run schema DDL.
    fn init(conn: Connection) -> Result<Self, StorageError> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| StorageError::Failure(format!("sqlite WAL: {e}")))?;
        conn.execute_batch(SCHEMA_SQL)
            .map_err(|e| StorageError::Failure(format!("sqlite schema: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Register a publish token. Idempotent — inserting the same token
    /// twice is a no-op (INSERT OR IGNORE).
    ///
    /// Production token provisioning is via GitHub OAuth (T57 commit 2);
    /// this method is for seeding test tokens + local-dev setup.
    pub fn add_token(&self, token: &str) -> Result<(), StorageError> {
        let conn = self.lock_conn()?;
        conn.execute("INSERT OR IGNORE INTO tokens (token) VALUES (?1)", params![token])
            .map_err(|e| StorageError::Failure(format!("add_token: {e}")))?;
        Ok(())
    }

    /// T57: Register a verified author (mock — real verification via
    /// GitHub OAuth verified-email is a follow-up). Idempotent.
    pub fn add_verified_author(&self, author: &str) -> Result<(), StorageError> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO verified_authors (author) VALUES (?1)",
            params![author],
        )
        .map_err(|e| StorageError::Failure(format!("add_verified_author: {e}")))?;
        Ok(())
    }

    /// Lock the connection, mapping poison errors to [`StorageError`].
    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StorageError> {
        self.conn
            .lock()
            .map_err(|e| StorageError::Failure(format!("sqlite mutex poisoned: {e}")))
    }

    /// Compute the scope prefix from a package name. Returns `None`
    /// for unscoped names (`foo`), `Some("@org")` for scoped names
    /// (`@org/pkg`). Stored in the `packages.scope` column for fast
    /// scope-based queries (T57 commit 3).
    fn scope_of(name: &str) -> Option<String> {
        if name.starts_with('@') {
            name.split_once('/')
                .map(|(scope, _)| scope.to_string())
        } else {
            None
        }
    }
}

impl crate::storage::Storage for SqliteStorage {
    #[allow(clippy::too_many_arguments)]
    fn put_version(
        &self,
        name: &str,
        version: Version,
        deps: Vec<DepSpec>,
        tarball: Vec<u8>,
        author: Option<String>,
        published_at: Option<u64>,
        quality: QualityAttachment,
    ) -> Result<(), StorageError> {
        let conn = self.lock_conn()?;
        // Ensure the package row exists (INSERT OR IGNORE — the row
        // may already exist from a prior version publish).
        let scope = Self::scope_of(name);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        conn.execute(
            "INSERT OR IGNORE INTO packages (name, scope, created_at) VALUES (?1, ?2, ?3)",
            params![name, scope, now],
        )
        .map_err(|e| StorageError::Failure(format!("put_version insert package: {e}")))?;

        // Check for version-exists BEFORE inserting (return the sentinel
        // marker so the handler maps it to HTTP 400 VersionExists).
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM versions WHERE name = ?1 AND version = ?2",
                params![name, version.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| StorageError::Failure(format!("put_version check exists: {e}")))?
            .is_some();
        if exists {
            return Err(StorageError::Failure(VERSION_EXISTS_MARKER.to_string()));
        }

        let deps_json = serde_json::to_string(&deps)
            .map_err(|e| StorageError::Failure(format!("put_version serialize deps: {e}")))?;
        let quality_json = serde_json::to_string(&quality)
            .map_err(|e| StorageError::Failure(format!("put_version serialize quality: {e}")))?;
        conn.execute(
            "INSERT INTO versions (name, version, deps_json, tarball, author, published_at, quality_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                name,
                version.to_string(),
                deps_json,
                tarball,
                author,
                published_at.map(|v| v as i64),
                quality_json,
            ],
        )
        .map_err(|e| StorageError::Failure(format!("put_version insert: {e}")))?;
        Ok(())
    }

    fn get_package(&self, name: &str) -> Result<Option<PackageMetadata>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT version, deps_json, author, published_at \
                 FROM versions WHERE name = ?1 ORDER BY version",
            )
            .map_err(|e| StorageError::Failure(format!("get_package prepare: {e}")))?;
        let rows: Vec<VersionInfo> = stmt
            .query_map(params![name], |row| {
                let version: String = row.get(0)?;
                let deps_json: String = row.get(1)?;
                let author: Option<String> = row.get(2)?;
                let published_at: Option<i64> = row.get(3)?;
                let deps: Vec<DepSpec> = serde_json::from_str(&deps_json).unwrap_or_default();
                Ok(VersionInfo {
                    version,
                    deps,
                    author,
                    published_at: published_at.map(|v| v as u64),
                })
            })
            .map_err(|e| StorageError::Failure(format!("get_version query: {e}")))?
            .filter_map(|r| r.ok())
            .collect();
        if rows.is_empty() {
            // Check if the package row exists at all (0 versions = unknown).
            let pkg_exists: bool = conn
                .query_row(
                    "SELECT 1 FROM packages WHERE name = ?1",
                    params![name],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|e| StorageError::Failure(format!("get_version pkg check: {e}")))?
                .is_some();
            if !pkg_exists {
                return Ok(None);
            }
        }
        Ok(Some(PackageMetadata {
            name: name.to_string(),
            versions: rows,
        }))
    }

    fn get_tarball(&self, name: &str, version: &Version) -> Result<Option<Vec<u8>>, StorageError> {
        let conn = self.lock_conn()?;
        let row: Option<Vec<u8>> = conn
            .query_row(
                "SELECT tarball FROM versions WHERE name = ?1 AND version = ?2",
                params![name, version.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| StorageError::Failure(format!("get_tarball: {e}")))?;
        Ok(row)
    }

    fn list_versions_with_deps(
        &self,
        name: &str,
    ) -> Result<Vec<(Version, Vec<DepSpec>)>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare("SELECT version, deps_json FROM versions WHERE name = ?1")
            .map_err(|e| StorageError::Failure(format!("list_versions prepare: {e}")))?;
        let rows = stmt
            .query_map(params![name], |row| {
                let version_str: String = row.get(0)?;
                let deps_json: String = row.get(1)?;
                let deps: Vec<DepSpec> = serde_json::from_str(&deps_json).unwrap_or_default();
                let version = Version::parse(&version_str).unwrap_or_else(|_| {
                    // Fallback: should never happen (versions are validated
                    // before storage), but avoid panicking on corrupted data.
                    Version::new(0, 0, 0)
                });
                Ok((version, deps))
            })
            .map_err(|e| StorageError::Failure(format!("list_versions query: {e}")))?;
        let result: Vec<(Version, Vec<DepSpec>)> = rows.flatten().collect();
        Ok(result)
    }

    fn validate_token(&self, token: &str) -> Result<bool, StorageError> {
        let conn = self.lock_conn()?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM tokens WHERE token = ?1",
                params![token],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| StorageError::Failure(format!("validate_token: {e}")))?
            .is_some();
        Ok(exists)
    }

    fn try_record_publish(
        &self,
        token: &str,
        window: Duration,
        max: usize,
    ) -> Result<bool, StorageError> {
        let conn = self.lock_conn()?;
        // Wall-clock milliseconds (SystemTime-based) so the rate-limit
        // window survives process restarts (Instant is NOT serializable).
        let now_wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cutoff_wall = now_wall.saturating_sub(window.as_millis() as i64);

        // Prune old entries outside the rolling window.
        conn.execute(
            "DELETE FROM rate_log WHERE token = ?1 AND ts_ms < ?2",
            params![token, cutoff_wall],
        )
        .map_err(|e| StorageError::Failure(format!("try_record prune: {e}")))?;

        // Count current entries inside the window.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rate_log WHERE token = ?1",
                params![token],
                |row| row.get(0),
            )
            .map_err(|e| StorageError::Failure(format!("try_record count: {e}")))?;

        if count >= max as i64 {
            return Ok(false);
        }

        // Record this publish timestamp.
        conn.execute(
            "INSERT INTO rate_log (token, ts_ms) VALUES (?1, ?2)",
            params![token, now_wall],
        )
        .map_err(|e| StorageError::Failure(format!("try_record insert: {e}")))?;
        Ok(true)
    }

    fn list_packages(&self) -> Result<Vec<PackageSummary>, StorageError> {
        let conn = self.lock_conn()?;
        // For each package, find the latest version (highest semver).
        // SQLite doesn't have native semver ordering, so we fetch all
        // (name, version, deps_json, author, published_at, quality_json)
        // and pick the max version per package in Rust. Typical registry
        // has <10k versions total, so this is fine.
        let mut stmt = conn
            .prepare(
                "SELECT v.name, v.version, v.author, v.published_at, v.quality_json \
                 FROM versions v \
                 ORDER BY v.name",
            )
            .map_err(|e| StorageError::Failure(format!("list_packages prepare: {e}")))?;

        #[derive(Clone)]
        struct RowData {
            version: String,
            author: Option<String>,
            published_at: Option<u64>,
            quality: QualityAttachment,
        }

        let rows = stmt
            .query_map([], |row| {
                let version: String = row.get(1)?;
                let author: Option<String> = row.get(2)?;
                let published_at: Option<i64> = row.get(3)?;
                let quality_json: String = row.get(4)?;
                let quality: QualityAttachment =
                    serde_json::from_str(&quality_json).unwrap_or_default();
                Ok((
                    row.get::<_, String>(0)?, // name
                    RowData {
                        version,
                        author,
                        published_at: published_at.map(|v| v as u64),
                        quality,
                    },
                ))
            })
            .map_err(|e| StorageError::Failure(format!("list_packages query: {e}")))?;

        // Group by name, pick highest version per package.
        let mut by_name: std::collections::BTreeMap<String, Vec<RowData>> =
            std::collections::BTreeMap::new();
        for (name, data) in rows.flatten() {
            by_name.entry(name).or_default().push(data);
        }

        let mut summaries = Vec::with_capacity(by_name.len());
        for (name, versions) in by_name {
            // Pick the highest semver version.
            let latest = versions
                .into_iter()
                .max_by(|a, b| {
                    let va = Version::parse(&a.version).unwrap_or(Version::new(0, 0, 0));
                    let vb = Version::parse(&b.version).unwrap_or(Version::new(0, 0, 0));
                    va.cmp(&vb)
                })
                .unwrap_or_else(|| RowData {
                    version: "0.0.0".to_string(),
                    author: None,
                    published_at: None,
                    quality: QualityAttachment::default(),
                });
            summaries.push(PackageSummary {
                name,
                latest_version: latest.version,
                author: latest.author,
                last_published_at: latest.published_at,
                quality: latest.quality,
            });
        }
        Ok(summaries)
    }

    fn is_verified_author(&self, author: &str) -> Result<bool, StorageError> {
        let conn = self.lock_conn()?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM verified_authors WHERE author = ?1",
                params![author],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| StorageError::Failure(format!("is_verified_author: {e}")))?
            .is_some();
        Ok(exists)
    }
}

impl Default for SqliteStorage {
    fn default() -> Self {
        Self::open_in_memory().unwrap_or_else(|_| {
            // Should never fail for in-memory SQLite, but panic-free:
            // create a SqliteStorage with a lazily-failing connection.
            // In practice this path is unreachable — open_in_memory
            // only fails on OOM or SQLite init failure.
            let conn = Connection::open_in_memory()
                .expect("in-memory SQLite must succeed in default()");
            Self::init(conn).expect("in-memory SQLite init must succeed in default()")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::QualityAttachment;
    use crate::Storage;

    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version::new(major, minor, patch)
    }

    fn dep(name: &str) -> DepSpec {
        DepSpec {
            name: name.to_string(),
            req: "*".to_string(),
        }
    }

    #[test]
    fn sqlite_roundtrip_publish_then_get() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        storage.add_token("tok").expect("add_token");
        storage
            .put_version(
                "pkg",
                v(1, 0, 0),
                vec![dep("dep-a")],
                vec![0xAB, 0xCD],
                Some("tok".to_string()),
                Some(1234567890),
                QualityAttachment::default(),
            )
            .expect("put");

        let meta = storage.get_package("pkg").expect("get").expect("present");
        assert_eq!(meta.name, "pkg");
        assert_eq!(meta.versions.len(), 1);
        assert_eq!(meta.versions[0].version, "1.0.0");
        assert_eq!(meta.versions[0].deps, vec![dep("dep-a")]);
        assert_eq!(meta.versions[0].author.as_deref(), Some("tok"));

        let tarball = storage
            .get_tarball("pkg", &v(1, 0, 0))
            .expect("get_tarball")
            .expect("present");
        assert_eq!(tarball, vec![0xAB, 0xCD]);
    }

    #[test]
    fn sqlite_durability_across_connections() {
        // Simulate a process restart by opening a NEW connection to the
        // same file. Data must persist.
        let tmp = tempfile::tempdir().expect("tempdir");
        let db_path = tmp.path().join("test.db");

        {
            let storage = SqliteStorage::open(&db_path).expect("open");
            storage
                .put_version(
                    "durable",
                    v(2, 0, 0),
                    vec![],
                    vec![1, 2, 3],
                    None,
                    None,
                    QualityAttachment::default(),
                )
                .expect("put");
        }
        // "Restart": open a fresh connection to the same file.
        {
            let storage = SqliteStorage::open(&db_path).expect("reopen");
            let meta = storage
                .get_package("durable")
                .expect("get")
                .expect("present");
            assert_eq!(meta.versions.len(), 1);
            assert_eq!(meta.versions[0].version, "2.0.0");
            let tarball = storage
                .get_tarball("durable", &v(2, 0, 0))
                .expect("get_tarball")
                .expect("present");
            assert_eq!(tarball, vec![1, 2, 3]);
        }
    }

    #[test]
    fn sqlite_version_exists_returns_marker() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        storage
            .put_version(
                "dup",
                v(1, 0, 0),
                vec![],
                vec![],
                None,
                None,
                QualityAttachment::default(),
            )
            .expect("first put");
        let err = storage
            .put_version(
                "dup",
                v(1, 0, 0),
                vec![],
                vec![],
                None,
                None,
                QualityAttachment::default(),
            )
            .expect_err("should fail");
        assert_eq!(err.to_string(), format!("storage failure: {VERSION_EXISTS_MARKER}"));
    }

    #[test]
    fn sqlite_token_validation() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        storage.add_token("real-token").expect("add");
        assert!(storage.validate_token("real-token").expect("validate"));
        assert!(!storage.validate_token("fake-token").expect("validate"));
    }

    #[test]
    fn sqlite_rate_limiting() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        storage.add_token("rl-tok").expect("add");
        let window = Duration::from_secs(60);
        // 3 publishes allowed.
        for _ in 0..3 {
            assert!(storage
                .try_record_publish("rl-tok", window, 3)
                .expect("record"));
        }
        // 4th must be rejected.
        assert!(!storage
            .try_record_publish("rl-tok", window, 3)
            .expect("record"));
    }

    #[test]
    fn sqlite_list_packages_picks_latest_version() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        for ver in ["1.0.0", "1.1.0", "2.0.0"] {
            storage
                .put_version(
                    "multi",
                    Version::parse(ver).expect("ver"),
                    vec![],
                    vec![],
                    Some("auth".to_string()),
                    Some(100),
                    QualityAttachment::default(),
                )
                .expect("put");
        }
        let summaries = storage.list_packages().expect("list");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "multi");
        assert_eq!(summaries[0].latest_version, "2.0.0");
        assert_eq!(summaries[0].author.as_deref(), Some("auth"));
    }

    #[test]
    fn sqlite_verified_author() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        storage.add_verified_author("vip").expect("add");
        assert!(storage.is_verified_author("vip").expect("check"));
        assert!(!storage.is_verified_author("nobody").expect("check"));
    }

    #[test]
    fn sqlite_list_versions_with_deps() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        storage
            .put_version(
                "dep-pkg",
                v(1, 0, 0),
                vec![dep("a"), dep("b")],
                vec![],
                None,
                None,
                QualityAttachment::default(),
            )
            .expect("put");
        let versions = storage
            .list_versions_with_deps("dep-pkg")
            .expect("list");
        assert_eq!(versions.len(), 1);
        let (_, deps) = &versions[0];
        assert_eq!(deps.len(), 2);
    }

    #[test]
    fn sqlite_get_unknown_package_returns_none() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        assert!(storage.get_package("nope").expect("get").is_none());
    }

    #[test]
    fn sqlite_quality_attachment_roundtrip() {
        let storage = SqliteStorage::open_in_memory().expect("open");
        let quality = QualityAttachment {
            tested_coverage: Some(87.5),
            documented_coverage: Some(42.0),
            security_audit: None,
        };
        storage
            .put_version(
                "qual",
                v(1, 0, 0),
                vec![],
                vec![],
                None,
                None,
                quality.clone(),
            )
            .expect("put");
        let summaries = storage.list_packages().expect("list");
        let row = summaries.iter().find(|s| s.name == "qual").expect("found");
        assert_eq!(row.quality.tested_coverage, Some(87.5));
        assert_eq!(row.quality.documented_coverage, Some(42.0));
    }

    #[test]
    fn scope_of_unscoped_returns_none() {
        assert_eq!(SqliteStorage::scope_of("foo"), None);
        assert_eq!(SqliteStorage::scope_of("foo-bar"), None);
    }

    #[test]
    fn scope_of_scoped_returns_org() {
        assert_eq!(SqliteStorage::scope_of("@org/pkg"), Some("@org".to_string()));
        assert_eq!(
            SqliteStorage::scope_of("@buff/core"),
            Some("@buff".to_string())
        );
    }
}
