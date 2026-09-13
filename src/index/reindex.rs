//! Reindex processing — takes a list of changed files and processes
//! them through the full pipeline: parse, entity sync, edge sync,
//! auto-link rebuild, stale marking.
//!
//! This is the processing layer. It does NOT do detection (that's
//! `detection.rs`) and does NOT manage the baseline commit (that's
//! `baseline.rs`). Callers decide what files to process and whether
//! to advance the baseline afterward.

use std::path::{Path, PathBuf};

use crate::index::{auto_link, code_graph, detection, parse, sync};
use crate::storage;

/// Result of an incremental code reindex.
#[derive(Debug, Default)]
pub struct ReindexResult {
    /// Whether the reindex was incremental (git diff) or fell back to full.
    pub incremental: bool,
    pub created: usize,
    pub updated: usize,
    pub marked_stale: usize,
    pub skipped: usize,
    /// IDs of code entities that were created or updated — used by
    /// the stale-knowledge flagging step to find affected observations/rules.
    pub changed_code_ids: Vec<String>,
    /// IDs of code entities that were marked stale (deleted files).
    pub deleted_code_ids: Vec<String>,
    pub synced_entity_ids: Vec<String>,
    /// Whether any source file failed to read (I/O error). Used by
    /// the caller to decide whether to advance the baseline commit.
    pub had_read_failures: bool,
}

/// Process a list of changed files through the full reindex pipeline.
///
/// Parses added/modified files, syncs entities to DB, re-extracts
/// structural edges, rebuilds auto-links, and marks deleted files'
/// entities as stale. Does NOT read or write the baseline commit —
/// that's the caller's responsibility.
pub fn reindex_files(
    storage: &storage::Storage,
    repo_root: &Path,
    changed: &[detection::ChangedFile],
) -> ReindexResult {
    let mut result = ReindexResult {
        incremental: true,
        ..Default::default()
    };

    // Separate changed files into added/modified (need parsing) and deleted.
    let (to_parse, deleted): (Vec<_>, Vec<_>) = changed
        .iter()
        .partition(|f| f.change != detection::ChangeType::Deleted);

    // Read and parse only changed files in a single pass (no lock held).
    let parse_paths: Vec<PathBuf> = to_parse.iter().map(|f| f.path.clone()).collect();
    let parsed = parse::parse_source_files(repo_root, &parse_paths);
    result.had_read_failures = parsed.had_read_failures;

    // Sync changed entities to DB (no full stale sweep).
    let sync_result = sync::sync_code_entities_incremental(storage, &parsed.entities_by_file);
    result.created = sync_result.created;
    result.updated = sync_result.updated;
    result.skipped = sync_result.skipped;
    result.synced_entity_ids = sync_result.synced_entity_ids.clone();
    result.changed_code_ids = sync_result.synced_entity_ids.clone();
    // Entities removed from changed files (renamed or deleted within
    // a file that still exists) are marked stale by the sync function.
    result.marked_stale = sync_result.marked_stale;
    result.deleted_code_ids = sync_result.removed_entity_ids;

    // Re-extract structural edges for changed files only (incremental).
    if !parsed.source_files_for_edges.is_empty() {
        code_graph::sync_code_edges_incremental(storage, &parsed.source_files_for_edges);
    }

    // Rebuild auto-links. Code entity names/paths may have changed,
    // so knowledge→code auto-references need to be resynced. This
    // is a full rebuild (clears and recreates all auto-link edges)
    // to ensure no stale links remain after incremental changes.
    auto_link::sync_auto_links(storage);

    // Mark deleted files' code entities as stale.
    let deleted_paths: Vec<PathBuf> = deleted.iter().map(|f| f.path.clone()).collect();
    if !deleted_paths.is_empty() {
        result.marked_stale += sync::mark_stale_for_deleted_files(storage, &deleted_paths);
        // Collect the IDs of stale-marked entities for knowledge flagging.
        // Chunk the query to stay below SQLite's 999-variable limit:
        // 4 type params + N path params per chunk, max 990 paths.
        let conn = storage.conn();
        let code_types = ["function", "class", "file", "module"];
        let type_placeholders = (0..code_types.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let deleted_path_strs: Vec<String> = deleted_paths
            .iter()
            .map(|p| crate::index::path_to_string(p))
            .collect();
        const PATH_CHUNK: usize = 990;
        for chunk in deleted_path_strs.chunks(PATH_CHUNK) {
            let path_placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT id FROM entities \
                 WHERE type IN ({type_placeholders}) AND status = 'stale' \
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
            if let Ok(mut stmt) = conn.prepare(&sql)
                && let Ok(rows) = stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0))
            {
                for row in rows.flatten() {
                    result.deleted_code_ids.push(row);
                }
            }
        }
    }

    result
}
