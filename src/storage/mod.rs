//! Storage layer — SQLite database with entity, edge, FTS5, vec0, and
//! events tables.
//!
//! Single `Connection` behind `std::sync::Mutex`. DB calls from async
//! MCP handlers go through `tokio::task::spawn_blocking` (Phase 7).

pub mod access;
pub mod crud;
pub mod edges;
pub mod embeddings;
pub mod events;
pub mod graph;
pub mod graph_queries;
pub mod mining;
pub mod mining_signals;
pub mod query;
pub mod schema;
pub mod status;
pub mod usage;

pub use crud::{Entity, EntityType};
pub use edges::Edge;
pub use events::{DomainEvent, EventType};
pub use schema::SCHEMA_VERSION;

use std::path::Path;
use std::sync::{Mutex, Once};

use rusqlite::{Connection, ffi::sqlite3_auto_extension};
use sqlite_vec::sqlite3_vec_init;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("illegal status transition: {from} → {to}")]
    IllegalTransition { from: String, to: String },
    #[error("entity not found: {0}")]
    EntityNotFound(String),
    #[error("invalid entity type: {0}")]
    InvalidEntityType(String),
    #[error("schema version mismatch: db has {db}, binary expects {expected}")]
    SchemaVersionMismatch { db: u32, expected: u32 },
    #[error("file operation failed: {0}")]
    File(String),
}

/// Ensures sqlite-vec extension is registered exactly once before any
/// Connection is opened.
static VEC_INIT: Once = Once::new();

pub(crate) fn ensure_vec_extension() {
    VEC_INIT.call_once(|| {
        // SAFETY: sqlite3_vec_init is the C entrypoint for the sqlite-vec
        // extension. sqlite3_auto_extension registers it so every new
        // Connection automatically loads vec0. This is the standard
        // pattern documented in the sqlite-vec Rust guide.
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut std::os::raw::c_char,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> std::os::raw::c_int,
            >(sqlite3_vec_init as *const ())));
        }
    });
}

/// Register connection-scoped SQL functions the query layer calls.
/// Scalar functions live on the Connection, not the schema, so every
/// open path — `Storage::open` and bare-Connection test harnesses —
/// must run this. `run_migrations` calls it for all of them.
///
/// `is_test_path` wraps `index::gitignore::is_test_file` — the single
/// test-file predicate. SQL filters call it instead of re-encoding
/// the convention as LIKE wildcards (where `_` and `%` silently
/// over-match production paths like `latest.go`/`testing.py`).
pub(crate) fn register_functions(conn: &Connection) -> Result<(), StorageError> {
    conn.create_scalar_function(
        "is_test_path",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let path: Option<String> = ctx.get(0)?;
            Ok(path.is_some_and(|p| crate::index::gitignore::is_test_file(&p)) as i64)
        },
    )?;
    Ok(())
}

/// The storage layer. Owns a single SQLite connection behind a Mutex.
///
/// Also owns a `file_mutex` that serializes canonical file
/// read-modify-write sequences (knowledge updates, stale flagging,
/// merging, contradiction recording, promotion, pruning). Callers
/// that read a canonical file, modify it, and write it back must
/// hold `file_lock()` for the entire sequence to prevent concurrent
/// writers from clobbering each other's changes.
///
/// For cross-process safety, `file_lock()` also acquires an OS-backed
/// advisory lock on a `.cogz/.lock` file. This prevents concurrent
/// CogZ processes (hooks, CLI, MCP server) from performing
/// read-modify-write sequences simultaneously.
pub struct Storage {
    conn: Mutex<Connection>,
    file_mutex: Mutex<()>,
    lock_file: Option<std::fs::File>,
}

impl Storage {
    /// Open or create a database at the given path.
    ///
    /// Enables WAL mode, registers sqlite-vec, runs migrations, and
    /// verifies the schema version. `embedding_dim` configures the
    /// vec0 virtual table dimension and must match the model.
    pub fn open(path: &Path, embedding_dim: usize) -> Result<Self, StorageError> {
        ensure_vec_extension();

        let conn = if path == Path::new(":memory:") {
            Connection::open_in_memory()?
        } else {
            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                tracing::warn!("failed to create DB directory {}: {}", parent.display(), e);
            }
            Connection::open(path)?
        };

        // WAL mode for crash recovery and checkpoint behavior.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Wait up to 5 seconds when the database is locked by another
        // process before returning SQLITE_BUSY. Without this, concurrent
        // background processes (reindex-bg, embed-bg) get immediate
        // "database is locked" errors instead of retrying.
        conn.pragma_update(None, "busy_timeout", "5000")?;

        schema::run_migrations(&conn, embedding_dim)?;
        schema::check_version(&conn)?;

        // Open a cross-process lock file alongside the DB. The lock
        // is acquired on demand via `file_lock()`. For in-memory DBs
        // (tests), no lock file is needed.
        let lock_file = if path == Path::new(":memory:") {
            None
        } else {
            let lock_path = path.parent().unwrap_or(Path::new(".")).join(".lock");
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&lock_path)
            {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(
                        "cannot open {} — cross-process write safety is off: {e}",
                        lock_path.display()
                    );
                    None
                }
            }
        };

        Ok(Self {
            conn: Mutex::new(conn),
            file_mutex: Mutex::new(()),
            lock_file,
        })
    }

    /// Open an in-memory database (for tests). Uses 768-dim embeddings.
    pub fn open_memory() -> Result<Self, StorageError> {
        Self::open(Path::new(":memory:"), 768)
    }

    /// Acquire the canonical-file write lock. Hold this guard across
    /// any read-modify-write sequence on canonical entity files to
    /// prevent concurrent writers from discarding each other's
    /// changes. Do NOT hold the DB connection guard simultaneously
    /// unless the operation requires both — release the file lock
    /// before long-running I/O.
    ///
    /// This acquires both an in-process mutex (fast path) and an
    /// OS-backed advisory lock on `.cogz/.lock` (cross-process safety).
    /// The OS lock is released when the guard is dropped.
    ///
    /// The wait is bounded: `Err` after ~10 s of contention — a stuck
    /// worker must not wedge every other process on this repo.
    /// Callers that can degrade (background/hook paths) should; write
    /// paths should propagate rather than proceed unlocked.
    pub fn file_lock(&self) -> std::io::Result<FileLockGuard<'_>> {
        self.file_lock_deadline(std::time::Duration::from_secs(10))
    }

    /// `file_lock` with an explicit deadline — kept `pub(crate)` so
    /// tests don't sit on the 10 s default.
    pub(crate) fn file_lock_deadline(
        &self,
        timeout: std::time::Duration,
    ) -> std::io::Result<FileLockGuard<'_>> {
        let mutex_guard = self.file_mutex.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref lock_file) = self.lock_file {
            lock_file_bounded(lock_file, timeout)?;
        }
        Ok(FileLockGuard {
            _mutex: mutex_guard,
            lock_file: self.lock_file.as_ref(),
        })
    }

    /// Access the underlying connection. For internal use within
    /// storage module functions. Recovers from a poisoned mutex
    /// rather than panicking — a panic in one task should not kill
    /// the entire server.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Database file size in bytes (0 for in-memory).
    pub fn db_size_bytes(&self) -> u64 {
        // Get the file path under the lock, then drop it before
        // doing filesystem I/O to avoid blocking other callers.
        let path = {
            let conn = self.conn();
            let path: Option<String> = conn
                .query_row("PRAGMA database_list", [], |r| r.get::<_, String>(2))
                .ok();
            path
        };
        match path {
            Some(ref p) if !p.is_empty() => std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
            _ => 0,
        }
    }
}

/// Open `<db_path_dir>/.lock` and take a bounded exclusive OS lock on
/// it, without opening a `Storage`. `cogz reset` uses this — opening
/// `Storage` would create + migrate the very DB it is about to
/// delete. The returned `File` holds the flock until dropped. Errs
/// after `timeout` of contention — same contract as `file_lock()`.
pub fn lock_repo_file(db_path: &Path) -> std::io::Result<std::fs::File> {
    let lock_path = db_path.parent().unwrap_or(Path::new(".")).join(".lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock_path)?;
    lock_file_bounded(&file, std::time::Duration::from_secs(10))?;
    Ok(file)
}

/// Bounded exclusive try-lock on an open lock file — the retry loop
/// behind `file_lock_deadline` and `lock_repo_file`.
pub(crate) fn lock_file_bounded(
    file: &std::fs::File,
    timeout: std::time::Duration,
) -> std::io::Result<()> {
    use fs2::FileExt;
    let start = std::time::Instant::now();
    let mut backoff = std::time::Duration::from_millis(10);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    // Windows try-lock failure is
                    // ERROR_LOCK_VIOLATION, not WouldBlock.
                    || (cfg!(windows) && e.raw_os_error() == Some(33)) =>
            {
                let elapsed = start.elapsed();
                if elapsed >= timeout {
                    return Err(e);
                }
                std::thread::sleep(backoff.min(timeout - elapsed));
                backoff = (backoff * 2).min(std::time::Duration::from_millis(250));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Guard for the canonical-file write lock. Releases both the
/// in-process mutex and the OS-backed advisory lock on drop.
pub struct FileLockGuard<'a> {
    _mutex: std::sync::MutexGuard<'a, ()>,
    lock_file: Option<&'a std::fs::File>,
}

impl Drop for FileLockGuard<'_> {
    fn drop(&mut self) {
        if let Some(lock_file) = self.lock_file {
            // fs2::FileExt::unlock — the trait is already in scope
            // from the `file_lock()` method above.
            let _ = fs2::FileExt::unlock(lock_file);
        }
    }
}

/// Set a metadata key-value pair in the `meta` table.
pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), StorageError> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Get a metadata value by key. Returns None if the key doesn't exist.
pub fn get_meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_memory_creates_schema() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        // Verify tables exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(count >= 5, "expected at least 5 tables, got {count}");
    }

    #[test]
    fn open_memory_schema_version() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn open_file_based_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let storage = Storage::open(&db_path, 768).unwrap();
        let conn = storage.conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn wal_mode_enabled() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        // In-memory DBs use "memory" journal, file DBs use "wal"
        assert!(mode == "wal" || mode == "memory", "got journal_mode={mode}");
    }

    #[test]
    fn file_lock_times_out_instead_of_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cogz.db");
        let first = Storage::open(&db_path, 768).unwrap();
        let second = Storage::open(&db_path, 768).unwrap();

        let _guard = first.file_lock().unwrap();
        // Two handles on the same .lock file contend across open file
        // descriptions — the second must Err, not park forever.
        let started = std::time::Instant::now();
        let result = second.file_lock_deadline(std::time::Duration::from_millis(120));
        assert!(result.is_err());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "bounded wait exceeded its deadline: {:?}",
            started.elapsed()
        );

        drop(_guard);
        assert!(second.file_lock().is_ok());
    }
}
