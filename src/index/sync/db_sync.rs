use std::collections::HashMap;

use crate::index::sync::{CodeSyncResult, ParsedEntity};
use crate::storage;

pub(crate) fn sync_entities_to_db(
    conn: &rusqlite::Connection,
    all_entities: &[ParsedEntity],
    result: &mut CodeSyncResult,
) {
    let ids: Vec<String> = all_entities.iter().map(|(id, _)| id.clone()).collect();
    let existing_map: HashMap<String, storage::crud::Entity> =
        match storage::crud::get_entities_batch(conn, &ids) {
            Ok(entities) => entities.into_iter().map(|e| (e.id.clone(), e)).collect(),
            Err(e) => {
                // Without the existing-entity map every entity would
                // be misclassified as new; abort rather than sync wrong.
                tracing::warn!("failed to fetch existing entities — skipping code sync: {e}");
                return;
            }
        };

    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut synced_ids: Vec<String> = Vec::new();

    // Wrap all inserts/updates in a single transaction to avoid
    // one fsync per row. unchecked_transaction() provides Drop-based
    // rollback on panic — if the code panics between BEGIN and COMMIT,
    // the Transaction's Drop impl rolls back automatically. This is
    // safer than raw execute_batch("BEGIN") which leaves the
    // transaction open on panic.
    let tx = match conn.unchecked_transaction() {
        Ok(tx) => tx,
        Err(e) => {
            tracing::warn!("failed to begin entity transaction — falling back to autocommit: {e}");
            sync_entities_loop(
                conn,
                all_entities,
                &existing_map,
                &mut created,
                &mut updated,
                &mut skipped,
                &mut synced_ids,
            );
            apply_counts(result, created, updated, skipped, synced_ids);
            return;
        }
    };

    sync_entities_loop(
        &tx,
        all_entities,
        &existing_map,
        &mut created,
        &mut updated,
        &mut skipped,
        &mut synced_ids,
    );

    if let Err(e) = tx.commit() {
        tracing::warn!("failed to commit entity transaction: {e}");
        // Transaction rolled back — discard created/updated counts
        // since those writes were undone. But skipped entities were
        // never modified (hash matched, no SQL executed), so the
        // skipped count is still accurate.
        result.skipped += skipped;
        return;
    }

    apply_counts(result, created, updated, skipped, synced_ids);
}

/// Inner loop for entity sync — works with either a `Connection` or
/// `Transaction` (both deref to `Connection`). Updates the count
/// counters and synced_ids in place.
fn sync_entities_loop(
    conn: &rusqlite::Connection,
    all_entities: &[ParsedEntity],
    existing_map: &HashMap<String, storage::crud::Entity>,
    created: &mut usize,
    updated: &mut usize,
    skipped: &mut usize,
    synced_ids: &mut Vec<String>,
) {
    for (id, entity) in all_entities {
        match existing_map.get(id) {
            Some(existing) => {
                if existing.status == "stale" {
                    let mut updated_entity = entity.clone();
                    updated_entity.status = "active".to_string();
                    updated_entity.created_at = existing.created_at.clone();
                    if let Err(e) = storage::crud::update_entity(conn, &updated_entity) {
                        tracing::warn!("failed to reactivate code entity {}: {}", id, e);
                        continue;
                    }
                    *updated += 1;
                    synced_ids.push(id.clone());
                    continue;
                }
                if existing.content_hash == entity.content_hash {
                    *skipped += 1;
                    continue;
                }
                let mut updated_entity = entity.clone();
                updated_entity.status = existing.status.clone();
                updated_entity.created_at = existing.created_at.clone();
                if let Err(e) = storage::crud::update_entity(conn, &updated_entity) {
                    tracing::warn!("failed to update code entity {}: {}", id, e);
                    continue;
                }
                *updated += 1;
                synced_ids.push(id.clone());
            }
            None => {
                if let Err(e) = storage::crud::insert_entity(conn, entity) {
                    tracing::warn!("failed to insert code entity {}: {}", id, e);
                    continue;
                }
                *created += 1;
                synced_ids.push(id.clone());
            }
        }
    }
}

/// Apply local counts to the sync result after a successful commit.
fn apply_counts(
    result: &mut CodeSyncResult,
    created: usize,
    updated: usize,
    skipped: usize,
    synced_ids: Vec<String>,
) {
    result.created += created;
    result.updated += updated;
    result.skipped += skipped;
    result.synced_entity_ids.extend(synced_ids);
}

/// Mark code entities as stale if their source file is no longer present.
///
/// Returns the number of entities marked stale. Uses a single batched
/// UPDATE query instead of fetching all entities and updating in a loop.
pub(crate) fn mark_stale_code_entities(
    conn: &rusqlite::Connection,
    current_files: &HashMap<String, Vec<String>>,
    failed_paths: &std::collections::HashSet<String>,
) -> usize {
    // Find active code entities whose file_path is NOT in current_files
    // and NOT in failed_paths. Files that failed to read are not deleted
    // — their entities must not be marked stale just because of an I/O
    // error, or valid edges will be lost and the baseline advanced.
    let code_types = ["function", "class", "file", "module"];
    let type_placeholders = (0..code_types.len())
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let sql = format!(
        "SELECT DISTINCT file_path FROM entities \
         WHERE status = 'active' AND type IN ({type_placeholders}) \
         AND file_path IS NOT NULL"
    );
    let params: Vec<&dyn rusqlite::ToSql> = code_types
        .iter()
        .map(|t| t as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("failed to query code entity file paths: {}", e);
            return 0;
        }
    };
    let rows = match stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0)) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to query code entity file paths: {}", e);
            return 0;
        }
    };

    let mut stale_paths: Vec<String> = Vec::new();
    for row in rows {
        match row {
            Ok(path) => {
                if !current_files.contains_key(&path) && !failed_paths.contains(&path) {
                    stale_paths.push(path);
                }
            }
            Err(e) => tracing::warn!("stale-scan row read failed: {e}"),
        }
    }

    storage::crud::mark_code_entities_stale_by_file_paths(conn, &stale_paths).unwrap_or_else(|e| {
        tracing::warn!("failed to mark code entities stale: {e}");
        0
    })
}

/// Mark code entities as stale for a set of deleted file paths.
/// Returns the count of entities marked stale. Uses a single batched
/// UPDATE query instead of fetching all entities and updating in a loop.
pub fn mark_stale_for_deleted_files(
    storage: &storage::Storage,
    deleted_paths: &[std::path::PathBuf],
) -> usize {
    let conn = storage.conn();
    let deleted: Vec<String> = deleted_paths
        .iter()
        .map(|p| crate::index::path_to_string(p))
        .collect();

    storage::crud::mark_code_entities_stale_by_file_paths(&conn, &deleted).unwrap_or_else(|e| {
        tracing::warn!("failed to mark deleted-file entities stale: {e}");
        0
    })
}

/// Mark code entities as stale when they exist in the DB for a changed
/// file but are no longer present in the new parse. This catches
/// renamed or removed entities within files that still exist on disk
/// (deleted files are handled by `mark_stale_for_deleted_files`).
///
/// Returns `(count, stale_entity_ids)` so callers can pass the IDs to
/// `flag_stale_knowledge` for downstream knowledge flagging.
pub(crate) fn mark_stale_for_removed_entities(
    conn: &rusqlite::Connection,
    file_to_entity_ids: &HashMap<String, Vec<String>>,
) -> (usize, Vec<String>) {
    if file_to_entity_ids.is_empty() {
        return (0, Vec::new());
    }

    let new_ids: std::collections::HashSet<String> = file_to_entity_ids
        .values()
        .flat_map(|ids| ids.iter().cloned())
        .collect();

    let code_types = ["function", "class", "file", "module"];
    let type_placeholders = (0..code_types.len())
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let file_paths: Vec<String> = file_to_entity_ids.keys().cloned().collect();
    let mut stale_ids: Vec<String> = Vec::new();

    // 4 type params + up to 995 file paths = 999 (SQLite variable limit).
    const CHUNK_SIZE: usize = 995;
    for chunk in file_paths.chunks(CHUNK_SIZE) {
        let path_placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id FROM entities \
             WHERE status = 'active' AND type IN ({type_placeholders}) \
             AND file_path IN ({path_placeholders})"
        );
        let mut params: Vec<&dyn rusqlite::ToSql> =
            Vec::with_capacity(code_types.len() + chunk.len());
        for t in &code_types {
            params.push(t);
        }
        for p in chunk {
            params.push(p);
        }
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("stale-scan prepare failed — chunk skipped: {e}");
                continue;
            }
        };
        let rows = match stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0)) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("stale-scan query failed — chunk skipped: {e}");
                continue;
            }
        };
        for row in rows {
            match row {
                Ok(id) if !new_ids.contains(&id) => stale_ids.push(id),
                Err(e) => tracing::warn!("stale-scan row read failed: {e}"),
                _ => {}
            }
        }
    }

    if stale_ids.is_empty() {
        return (0, Vec::new());
    }

    // Batch-mark stale via the storage layer (no direct SQL outside storage/).
    let count = match storage::crud::mark_entities_stale_by_ids(conn, &stale_ids) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("failed to mark removed entities stale: {}", e);
            return (0, Vec::new());
        }
    };

    (count, stale_ids)
}
