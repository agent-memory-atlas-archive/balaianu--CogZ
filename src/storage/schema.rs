//! Schema definitions, migrations, and version tracking.

use rusqlite::Connection;

use super::StorageError;

/// Current schema version. Increment when migrations are added.
/// Stored in `PRAGMA user_version`.
pub const SCHEMA_VERSION: u32 = 10;

/// Run all migrations to bring the database up to `SCHEMA_VERSION`.
///
/// Migrations are forward-only. On a fresh database, all tables are
/// created. On an existing database, only new migrations run.
///
/// `embedding_dim` is used for the vec0 virtual table dimension.
/// It must match the configured model dimension.
pub fn run_migrations(conn: &Connection, embedding_dim: usize) -> Result<(), StorageError> {
    // Connection-scoped functions the query layer relies on. Registered
    // before the version check — an already-current DB still needs them.
    crate::storage::register_functions(conn)?;

    // Migrations run inside one IMMEDIATE transaction: the write lock
    // is taken before `user_version` is read, so a second opener can't
    // race the version check, and a failed migration rolls back fully
    // instead of leaving half-applied DDL with the old version stamp.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = migrate_inner(conn, embedding_dim);
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn migrate_inner(conn: &Connection, embedding_dim: usize) -> Result<(), StorageError> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if current >= SCHEMA_VERSION {
        return Ok(());
    }

    if current < 1 {
        migrate_v1(conn, embedding_dim)?;
    }

    if current < 2 {
        migrate_v2(conn)?;
    }

    if current < 3 {
        migrate_v3(conn, embedding_dim)?;
    }

    if current < 4 {
        migrate_v4(conn)?;
    }

    if current < 5 {
        migrate_v5(conn)?;
    }

    if current < 6 {
        migrate_v6(conn)?;
    }

    if current < 7 {
        migrate_v7(conn)?;
    }

    if current < 8 {
        migrate_v8(conn)?;
    }

    if current < 9 {
        migrate_v9(conn)?;
    }

    if current < 10 {
        migrate_v10(conn)?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Verify the database schema version matches what the binary expects.
pub fn check_version(conn: &Connection) -> Result<(), StorageError> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current != SCHEMA_VERSION {
        return Err(StorageError::SchemaVersionMismatch {
            db: current,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

/// Migration v1: initial schema — all tables, indexes, FTS5 triggers.
fn migrate_v1(conn: &Connection, embedding_dim: usize) -> Result<(), StorageError> {
    // --- Entities table ---
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS entities (
            id           TEXT PRIMARY KEY,
            type         TEXT NOT NULL,
            title        TEXT,
            content      TEXT NOT NULL,
            properties   TEXT DEFAULT '{}',
            file_path    TEXT,
            status       TEXT DEFAULT 'active',
            content_hash TEXT,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_entities_type      ON entities(type);
        CREATE INDEX IF NOT EXISTS idx_entities_status    ON entities(status);
        CREATE INDEX IF NOT EXISTS idx_entities_file_path ON entities(file_path);
        "#,
    )?;

    // --- FTS5 external content table ---
    conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
            title,
            content,
            content='entities',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );
        "#,
    )?;

    // --- FTS5 sync triggers ---
    conn.execute_batch(
        r#"
        CREATE TRIGGER IF NOT EXISTS entities_fts_ai AFTER INSERT ON entities BEGIN
            INSERT INTO entities_fts(rowid, title, content)
            VALUES (new.rowid, new.title, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS entities_fts_ad AFTER DELETE ON entities BEGIN
            INSERT INTO entities_fts(entities_fts, rowid, title, content)
            VALUES('delete', old.rowid, old.title, old.content);
        END;

        CREATE TRIGGER IF NOT EXISTS entities_fts_au AFTER UPDATE ON entities BEGIN
            INSERT INTO entities_fts(entities_fts, rowid, title, content)
            VALUES('delete', old.rowid, old.title, old.content);
            INSERT INTO entities_fts(rowid, title, content)
            VALUES (new.rowid, new.title, new.content);
        END;
        "#,
    )?;

    // --- Edges table ---
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS edges (
            source_id   TEXT NOT NULL REFERENCES entities(id),
            target_id   TEXT NOT NULL REFERENCES entities(id),
            edge_type   TEXT NOT NULL,
            weight      REAL DEFAULT 1.0,
            created_at  TEXT NOT NULL,
            PRIMARY KEY (source_id, target_id, edge_type)
        );

        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id, edge_type);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id, edge_type);
        "#,
    )?;

    // --- Embeddings (separate vec0 tables for code and knowledge) ---
    // Code and knowledge entities use different embedding models with
    // incompatible vector spaces even at the same dimensionality. Separate
    // tables ensure KNN only compares vectors within the same space.
    let code_vec0 = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS code_embeddings USING vec0(\n            embedding FLOAT[{}],\n            entity_id TEXT\n        );",
        embedding_dim
    );
    conn.execute_batch(&code_vec0)?;

    let knowledge_vec0 = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_embeddings USING vec0(\n            embedding FLOAT[{}],\n            entity_id TEXT\n        );",
        embedding_dim
    );
    conn.execute_batch(&knowledge_vec0)?;

    // --- Events table ---
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            id          INTEGER PRIMARY KEY,
            event_type  TEXT NOT NULL,
            entity_id   TEXT REFERENCES entities(id),
            payload     TEXT DEFAULT '{}',
            created_at  TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_events_type   ON events(event_type);
        CREATE INDEX IF NOT EXISTS idx_events_entity ON events(entity_id);
        "#,
    )?;

    // --- Entity access tracking (derived, not in canonical files) ---
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS entity_access (
            entity_id    TEXT PRIMARY KEY REFERENCES entities(id),
            access_count INTEGER DEFAULT 0,
            last_accessed TEXT
        );
        "#,
    )?;

    Ok(())
}

/// Migration v2: meta table for key-value metadata (last_index, etc.).
fn migrate_v2(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

/// Migration v3: split entity_embeddings into code_embeddings and
/// knowledge_embeddings. Existing embeddings are migrated to the
/// correct table based on entity type, then the old table is dropped.
///
/// On fresh databases (v1 already creates the two new tables), the
/// old table doesn't exist so the migration is a no-op.
fn migrate_v3(conn: &Connection, embedding_dim: usize) -> Result<(), StorageError> {
    // Ensure the new tables exist (idempotent — v1 already creates them
    // on fresh databases, but this covers databases at v2 that predate
    // the split).
    let code_vec0 = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS code_embeddings USING vec0(\n            embedding FLOAT[{}],\n            entity_id TEXT\n        );",
        embedding_dim
    );
    conn.execute_batch(&code_vec0)?;

    let knowledge_vec0 = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_embeddings USING vec0(\n            embedding FLOAT[{}],\n            entity_id TEXT\n        );",
        embedding_dim
    );
    conn.execute_batch(&knowledge_vec0)?;

    // Migrate existing embeddings from the old single table if it exists.
    // vec0 tables don't support INSERT INTO ... SELECT across virtual
    // tables, so we read all embeddings in Rust and re-insert them.
    let has_old_table: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='entity_embeddings'")
        .map_err(StorageError::from)?
        .exists([])?;

    if has_old_table {
        use crate::storage::crud::EntityType;
        use zerocopy::IntoBytes;

        // Read all (entity_id, embedding) pairs from the old table.
        let rows: Vec<(String, Vec<u8>, String)> = {
            let mut stmt = conn.prepare(
                "SELECT e.entity_id, e.embedding, ent.type
                 FROM entity_embeddings e
                 JOIN entities ent ON ent.id = e.entity_id",
            )?;
            let row_iter = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            let mut rows = Vec::new();
            let mut dropped = 0usize;
            for row in row_iter {
                match row {
                    Ok(r) => rows.push(r),
                    Err(e) => {
                        dropped += 1;
                        tracing::warn!("v3 migration skipping unreadable embedding row: {e}");
                    }
                }
            }
            if dropped > 0 {
                tracing::warn!(
                    "v3 migration dropped {dropped} embedding rows — affected entities will be missing from vector search until re-embedded"
                );
            }
            rows
        };

        for (entity_id, blob, entity_type) in rows {
            let is_code = EntityType::parse(&entity_type)
                .map(|t| t.is_code())
                .unwrap_or(false);
            let table = if is_code {
                "code_embeddings"
            } else {
                "knowledge_embeddings"
            };
            let sql = format!(
                "INSERT INTO {}(entity_id, embedding) VALUES (?1, ?2)",
                table
            );
            // blob is already raw f32 bytes — re-insert as-is.
            let _: usize = conn.execute(&sql, rusqlite::params![entity_id, blob.as_bytes()])?;
        }

        // Drop the old table.
        conn.execute_batch("DROP TABLE IF EXISTS entity_embeddings")?;
    }

    Ok(())
}

/// Migration v4: usage instrumentation. `deliveries` tracks each batch
/// of entities shown to the agent (a context pack or a search result
/// set); `entity_usage` tracks the per-entity outcome — `pending`
/// until the agent touches it (`hit`) or the next delivery boundary
/// closes it (`miss`). Derived state: both tables are disposable and
/// rebuilt from agent activity, never from canonical files.
fn migrate_v4(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS deliveries (
            id          INTEGER PRIMARY KEY,
            kind        TEXT NOT NULL,
            event_id    INTEGER,
            closed      INTEGER DEFAULT 0,
            created_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS entity_usage (
            id          INTEGER PRIMARY KEY,
            delivery_id INTEGER NOT NULL REFERENCES deliveries(id),
            entity_id   TEXT NOT NULL REFERENCES entities(id),
            outcome     TEXT NOT NULL DEFAULT 'pending',
            created_at  TEXT NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_unique
            ON entity_usage(delivery_id, entity_id);
        CREATE INDEX IF NOT EXISTS idx_usage_delivery ON entity_usage(delivery_id);
        CREATE INDEX IF NOT EXISTS idx_usage_entity   ON entity_usage(entity_id);
        CREATE INDEX IF NOT EXISTS idx_usage_outcome  ON entity_usage(outcome);
        "#,
    )?;
    Ok(())
}

/// Migration v5: delivery tiers. `entity_usage.tier` records how an
/// entity was delivered — 'baseline' (Tier-0 orientation), 'full'
/// (Tier-1 content), 'pointer' (Tier-2 index entry) — so hit rates
/// can be measured per tier. Existing rows default to 'full', which
/// is what they were.
fn migrate_v5(conn: &Connection) -> Result<(), StorageError> {
    // ALTER TABLE has no IF NOT EXISTS — check the column first so a
    // rolled-back user_version doesn't fail on re-migration.
    let has_tier = conn
        .prepare("PRAGMA table_info(entity_usage)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .any(|name| name.ok().as_deref() == Some("tier"));
    if !has_tier {
        conn.execute_batch(
            "ALTER TABLE entity_usage ADD COLUMN tier TEXT NOT NULL DEFAULT 'full';",
        )?;
    }
    Ok(())
}

/// Migration v6: knowledge drift tracking. `entity_drift` records
/// per-reference divergence between a knowledge entity's
/// `verified_against` provenance (canonical frontmatter) and the
/// referenced code entity's current content hash. Derived state —
/// recomputed on every index; never written to canonical files.
fn migrate_v6(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS entity_drift (
            entity_id     TEXT NOT NULL REFERENCES entities(id),
            code_id       TEXT NOT NULL,
            verified_hash TEXT,
            current_hash  TEXT,
            cause         TEXT NOT NULL,
            PRIMARY KEY (entity_id, code_id)
        );

        CREATE INDEX IF NOT EXISTS idx_drift_entity ON entity_drift(entity_id);
        "#,
    )?;
    Ok(())
}

/// Migration v7: git co-change memory. `cochange` records which
/// commit-message terms travel with which entities — one row per
/// (term, entity, commit) so queries can bound the history used.
/// Mined from `git log -p -U0`: a commit's hunk ranges intersected
/// with current entity line ranges. Derived state — rebuilt by
/// `cogz index`; never written to canonical files.
fn migrate_v7(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cochange (
            term      TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            commit_id TEXT NOT NULL,
            commit_ts INTEGER NOT NULL,
            PRIMARY KEY (term, entity_id, commit_id)
        );

        CREATE INDEX IF NOT EXISTS idx_cochange_term      ON cochange(term);
        CREATE INDEX IF NOT EXISTS idx_cochange_entity_ts ON cochange(entity_id, commit_ts);
        "#,
    )?;
    Ok(())
}

/// Migration v8: telemetry timestamp normalization + retention
/// indexes. Rows written before the format unification carry
/// 'YYYY-MM-DD HH:MM:SS', which forced every cutoff filter to wrap
/// the column in datetime() — unindexable. Normalize them to the
/// RFC 3339 '+00:00' form all writers now emit, then plain string
/// comparison is both correct and index-servable.
fn migrate_v8(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        r#"
        UPDATE events SET created_at = replace(created_at, ' ', 'T') || '+00:00'
         WHERE created_at LIKE '% %';
        UPDATE deliveries SET created_at = replace(created_at, ' ', 'T') || '+00:00'
         WHERE created_at LIKE '% %';
        UPDATE entity_usage SET created_at = replace(created_at, ' ', 'T') || '+00:00'
         WHERE created_at LIKE '% %';

        CREATE INDEX IF NOT EXISTS idx_events_created_at      ON events(created_at);
        CREATE INDEX IF NOT EXISTS idx_events_type_created    ON events(event_type, created_at);
        CREATE INDEX IF NOT EXISTS idx_deliveries_created_at  ON deliveries(created_at);
        "#,
    )?;
    Ok(())
}

/// Migration v9: `vec_rowid` map — entity_id → vec0 rowid per
/// embedding space. vec0 auxiliary columns carry no index, so every
/// `WHERE entity_id = ?` on `code_embeddings`/`knowledge_embeddings`
/// was a full scan of the vector table. All entity-scoped embedding
/// reads/deletes now resolve the rowid through this regular indexed
/// table and hit vec0 by rowid only; the vec0 scan happens exactly
/// once, here, at backfill.
fn migrate_v9(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS vec_rowid (
            entity_id TEXT NOT NULL,
            space     TEXT NOT NULL,
            vec_rowid INTEGER NOT NULL,
            PRIMARY KEY (entity_id, space)
        );
        CREATE INDEX IF NOT EXISTS idx_vec_rowid_rowid ON vec_rowid(space, vec_rowid);

        INSERT OR IGNORE INTO vec_rowid (entity_id, space, vec_rowid)
            SELECT entity_id, 'code', rowid FROM code_embeddings;
        INSERT OR IGNORE INTO vec_rowid (entity_id, space, vec_rowid)
            SELECT entity_id, 'knowledge', rowid FROM knowledge_embeddings;
        "#,
    )?;
    Ok(())
}

/// Test hook: re-run the v9 backfill on a connection whose vec0 rows
/// were written without map entries (pre-v9 databases).
#[cfg(test)]
pub(crate) fn migrate_v9_for_test(conn: &Connection) {
    migrate_v9(conn).unwrap();
}

/// Migration v10: `entities.search_terms` — camelCase/PascalCase/digit
/// subtokens derived from the entity title at write time, indexed as a
/// third FTS column. unicode61 treats `verifyAuthToken` as one opaque
/// token; the split subtokens let FTS match "auth" and "token"
/// directly. Existing rows keep '' — identical behavior until reindex.
/// The FTS table is external-content, so it is dropped, recreated with
/// the new column list, and rebuilt from `entities`.
fn migrate_v10(conn: &Connection) -> Result<(), StorageError> {
    let has_col: bool = conn
        .prepare("PRAGMA table_info(entities)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .any(|name| name.ok().as_deref() == Some("search_terms"));
    if !has_col {
        conn.execute_batch(
            "ALTER TABLE entities ADD COLUMN search_terms TEXT NOT NULL DEFAULT '';",
        )?;
    }
    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS entities_fts_ai;
        DROP TRIGGER IF EXISTS entities_fts_ad;
        DROP TRIGGER IF EXISTS entities_fts_au;
        DROP TABLE IF EXISTS entities_fts;

        CREATE VIRTUAL TABLE entities_fts USING fts5(
            title,
            content,
            search_terms,
            content='entities',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );

        CREATE TRIGGER entities_fts_ai AFTER INSERT ON entities BEGIN
            INSERT INTO entities_fts(rowid, title, content, search_terms)
            VALUES (new.rowid, new.title, new.content, new.search_terms);
        END;

        CREATE TRIGGER entities_fts_ad AFTER DELETE ON entities BEGIN
            INSERT INTO entities_fts(entities_fts, rowid, title, content, search_terms)
            VALUES('delete', old.rowid, old.title, old.content, old.search_terms);
        END;

        CREATE TRIGGER entities_fts_au AFTER UPDATE ON entities BEGIN
            INSERT INTO entities_fts(entities_fts, rowid, title, content, search_terms)
            VALUES('delete', old.rowid, old.title, old.content, old.search_terms);
            INSERT INTO entities_fts(rowid, title, content, search_terms)
            VALUES (new.rowid, new.title, new.content, new.search_terms);
        END;

        INSERT INTO entities_fts(entities_fts) VALUES('rebuild');
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
