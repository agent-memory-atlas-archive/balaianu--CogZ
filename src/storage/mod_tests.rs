//! Tests for storage wiring.

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
