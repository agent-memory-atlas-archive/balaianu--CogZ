//! Tests for schema migrations.

use super::*;

#[test]
fn fresh_db_has_all_tables() {
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(tables.contains(&"entities".to_string()));
    assert!(tables.contains(&"edges".to_string()));
    assert!(tables.contains(&"events".to_string()));
    assert!(tables.contains(&"meta".to_string()));
    assert!(tables.contains(&"deliveries".to_string()));
    assert!(tables.contains(&"entity_usage".to_string()));
    // FTS5 and vec0 tables appear as virtual tables
    assert!(tables.iter().any(|t| t.contains("entities_fts")));
    assert!(tables.iter().any(|t| t.contains("code_embeddings")));
    assert!(tables.iter().any(|t| t.contains("knowledge_embeddings")));
}

#[test]
fn fts_triggers_sync_on_insert() {
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();

    conn.execute(
        "INSERT INTO entities (id, type, title, content, properties, status, created_at, updated_at)
         VALUES ('test-1', 'observation', 'Test Title', ' searchable content here ', '{}', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fts_count, 1);
}

#[test]
fn fts_triggers_sync_on_delete() {
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();

    conn.execute(
        "INSERT INTO entities (id, type, title, content, properties, status, created_at, updated_at)
         VALUES ('test-1', 'observation', 'Test', 'content here', '{}', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    conn.execute("DELETE FROM entities WHERE id = 'test-1'", [])
        .unwrap();

    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fts_count, 0);
}

#[test]
fn fts_triggers_sync_on_update() {
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();

    conn.execute(
        "INSERT INTO entities (id, type, title, content, properties, status, created_at, updated_at)
         VALUES ('test-1', 'observation', 'Old Title', 'old content', '{}', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    conn.execute(
        "UPDATE entities SET title = 'New Title', content = 'new content' WHERE id = 'test-1'",
        [],
    )
    .unwrap();

    let title: String = conn
        .query_row(
            "SELECT title FROM entities_fts WHERE entities_fts MATCH 'new'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "New Title");
}

#[test]
fn migrations_are_idempotent_and_forward_only() {
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();
    // Re-running the complete set is a no-op.
    run_migrations(&conn, 768).unwrap();

    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    // Simulated v3 database upgrade: roll the version back and
    // re-run the full migration path — v4 creates the usage
    // tables, v5 adds the tier column.
    conn.pragma_update(None, "user_version", 3u32).unwrap();
    run_migrations(&conn, 768).unwrap();
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    let tier_col: bool = conn
        .prepare("PRAGMA table_info(entity_usage)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .any(|name| name.ok().as_deref() == Some("tier"));
    assert!(tier_col, "entity_usage should have a tier column");
}

/// A mid-migration failure must roll back the whole transaction:
/// no partial tables, `user_version` untouched — a second opener
/// retries cleanly instead of colliding with half-applied DDL.
#[test]
fn failed_migration_rolls_back() {
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    // v1–v4 CREATEs all use IF NOT EXISTS — a same-named view is
    // skipped by them, but v5's `ALTER TABLE entity_usage` cannot
    // alter a view and fails after four migrations of DDL ran.
    conn.execute_batch("CREATE VIEW entity_usage AS SELECT 1 AS x")
        .unwrap();

    assert!(run_migrations(&conn, 768).is_err());

    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0, "rolled-back migration bumps no version");
    let entities: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'entities'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(entities, 0, "partial migration must roll back");

    // And the sabotage cleared, the retry completes.
    conn.execute_batch("DROP VIEW entity_usage").unwrap();
    run_migrations(&conn, 768).unwrap();
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn v8_normalizes_legacy_timestamps_and_adds_indexes() {
    crate::storage::ensure_vec_extension();
    crate::storage::ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();

    // Rows like this existed pre-v8 — re-running the migration's
    // normalization is what converts them (idempotent by design).
    conn.execute(
        "INSERT INTO events (event_type, created_at) VALUES ('x', '2024-01-02 03:04:05')",
        [],
    )
    .unwrap();
    migrate_v8(&conn).unwrap();
    let ts: String = conn
        .query_row(
            "SELECT created_at FROM events WHERE event_type = 'x'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ts, "2024-01-02T03:04:05+00:00");

    for name in [
        "idx_events_created_at",
        "idx_events_type_created",
        "idx_deliveries_created_at",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "missing index {name}");
    }
}
