//! Code indexing — tree-sitter parsing, gitignore-aware scanning,
//! and code entity synchronization.
//!
//! Phase 8: extract functions, classes, files, and modules from
//! source code into the entity graph with structural edges.

pub mod auto_link;
pub mod baseline;
pub mod code_graph;
pub mod detection;
pub mod drift;
pub mod git_diff;
pub mod gitignore;
pub mod parse;
pub mod reindex;
pub mod stale_flagging;
pub mod sync;
pub mod tree_sitter;

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::storage;
use crate::storage::crud::EntityType;

/// Normalize a path to use forward slashes regardless of platform.
/// Git stores paths with forward slashes internally; we do the same
/// so that code entity UUIDs are consistent across Linux, macOS, and
/// Windows. Without this, `src\main.rs` on Windows would produce a
/// different UUID than `src/main.rs` on Linux, breaking cross-platform
/// team collaboration.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Convert a Path to a normalized string with forward slashes.
pub fn path_to_string(path: &Path) -> String {
    normalize_path(&path.to_string_lossy())
}

pub use gitignore::is_test_file;
pub use reindex::ReindexResult;
pub use sync::{CodeSyncResult, mark_stale_for_deleted_files};

/// Index code entities from the repository.
///
/// Scans source files (respecting `.gitignore` + `[index].allow`),
/// parses them with tree-sitter, syncs code entities to the DB,
/// and extracts structural edges (calls, imports, extends).
///
/// Returns the sync result with counts for the CLI output.
pub fn index_code(storage: &storage::Storage, repo_root: &Path, config: &Config) -> CodeSyncResult {
    // Phase 1: scan for source files (no lock held).
    let source_paths = gitignore::scan_source_files(&gitignore::ScanConfig {
        root: repo_root,
        allow: &config.index.allow,
        deny: &config.index.deny,
    });

    // Phase 2: read and parse all source files in a single pass.
    // extract_all produces both entities and raw edges from one AST
    // walk, eliminating the need for a second parse during edge sync.
    let parsed = parse::parse_source_files(repo_root, &source_paths);

    // Phase 3: sync code entities to DB (lock held). Pass failed_paths
    // so that stale marking excludes files that failed to read — their
    // entities should not be marked stale just because of an I/O error.
    let mut result =
        sync::sync_code_entities(storage, &parsed.entities_by_file, &parsed.failed_paths);
    result.failed_files = parsed.failed_paths.len();

    // Phase 4: sync structural edges (lock held).
    // Skip the edge rebuild when any source file failed to read —
    // sync_code_edges deletes all existing edges before rebuilding,
    // which would remove outgoing edges from preserved entities.
    // Aborting the full rebuild keeps the previous edge set intact
    // until the next successful full index.
    if parsed.failed_paths.is_empty() {
        code_graph::sync_code_edges(storage, &parsed.entities_by_file, &parsed.raw_edges_by_file);
    } else {
        tracing::warn!(
            "skipping structural edge rebuild due to {} failed source read(s)",
            parsed.failed_paths.len()
        );
    }

    // Phase 5: auto-link knowledge entities to code entities.
    let link_count = auto_link::sync_auto_links(storage);
    if link_count > 0 {
        tracing::info!("auto-linked {} knowledge→code edges", link_count);
    }

    result
}

/// Count code entities by type for status reporting.
pub fn count_code_entities(conn: &rusqlite::Connection) -> (usize, usize, usize, usize) {
    let count = |entity_type: &str| -> usize {
        conn.query_row(
            "SELECT COUNT(*) FROM entities WHERE type = ?1 AND status = 'active'",
            rusqlite::params![entity_type],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    };

    (
        count(EntityType::Function.as_str()),
        count(EntityType::Class.as_str()),
        count(EntityType::File.as_str()),
        count(EntityType::Module.as_str()),
    )
}

/// Incremental code reindex — only re-parses files that changed since
/// the last index (detected via git diff). Falls back to a full
/// `index_code` if git is unavailable or no baseline commit is stored.
///
/// After syncing changed entities, records the current HEAD SHA as
/// the new baseline for the next reindex.
pub fn reindex_code(
    storage: &storage::Storage,
    repo_root: &Path,
    config: &Config,
) -> ReindexResult {
    let baseline = baseline::read_baseline(storage);

    let changed = detection::detect_changed_files(repo_root, baseline.as_deref());

    match changed {
        Some(files) if !files.is_empty() => {
            let result = reindex::reindex_files(storage, repo_root, &files);
            // Advance the baseline only if all changed files were
            // successfully read. On partial failure, preserve the
            // previous baseline so the next reindex retries them.
            if baseline::should_update_baseline(result.had_read_failures) {
                baseline::update_baseline(storage, repo_root);
            }
            result
        }
        Some(_files) => {
            // No changes — just update the baseline commit.
            baseline::update_baseline(storage, repo_root);
            ReindexResult {
                incremental: true,
                ..Default::default()
            }
        }
        None => {
            // No git or no baseline — full scan.
            let result = index_code(storage, repo_root, config);
            // The full scan marks deleted code entities as stale via
            // mark_stale_code_entities, but CodeSyncResult doesn't
            // return their IDs. Query them so flag_stale_knowledge can
            // be notified — otherwise knowledge referencing deleted
            // code silently remains active in the no-git path.
            let deleted_code_ids = if result.marked_stale > 0 {
                query_stale_code_ids(storage)
            } else {
                Vec::new()
            };
            ReindexResult {
                incremental: false,
                created: result.created,
                updated: result.updated,
                marked_stale: result.marked_stale,
                skipped: result.skipped,
                synced_entity_ids: result.synced_entity_ids.clone(),
                changed_code_ids: result.synced_entity_ids,
                deleted_code_ids,
                had_read_failures: result.failed_files > 0,
            }
        }
    }
}

/// Reindex a single source file. Used by hooks when a file_save event
/// provides the exact file path — skips git diff and baseline update.
///
/// Does NOT update the baseline commit. The next `cogz reindex` will
/// catch this file again via git diff, which is cheap and safe.
pub fn reindex_single_file(
    storage: &storage::Storage,
    repo_root: &Path,
    file_path: &str,
) -> ReindexResult {
    let path = std::path::PathBuf::from(file_path);
    // Hooks report absolute paths; entities must key on repo-relative
    // paths or every save mints a duplicate entity set under different
    // UUIDv5s. Components-normalize the relative branch too — a
    // `./src/foo.py` input must land on `src/foo.py`'s UUID.
    let rel = if path.is_absolute() {
        let root_abs = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
        let stripped = path
            .strip_prefix(&root_abs)
            .map(PathBuf::from)
            .ok()
            .or_else(|| {
                std::fs::canonicalize(&path)
                    .ok()
                    .and_then(|abs| abs.strip_prefix(&root_abs).ok().map(PathBuf::from))
            });
        match stripped {
            Some(r) => r,
            None => {
                tracing::warn!("file_save reindex: {} outside repo root", file_path);
                return ReindexResult::default();
            }
        }
    } else {
        path.components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect()
    };
    let changed = vec![detection::ChangedFile {
        path: rel,
        change: detection::ChangeType::Modified,
    }];
    reindex::reindex_files(storage, repo_root, &changed)
}

/// Query all stale code entity IDs (function, class, file, module).
/// Used by the full-scan reindex fallback to populate
/// `deleted_code_ids` so `flag_stale_knowledge` can be notified about
/// code entities that were marked stale because their source files no
/// longer exist on disk.
fn query_stale_code_ids(storage: &storage::Storage) -> Vec<String> {
    let conn = storage.conn();
    let code_types = ["function", "class", "file", "module"];
    let placeholders = (0..code_types.len())
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id FROM entities \
         WHERE type IN ({placeholders}) AND status = 'stale'"
    );
    let params: Vec<&dyn rusqlite::ToSql> = code_types
        .iter()
        .map(|t| t as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("failed to query stale code IDs: {}", e);
            return Vec::new();
        }
    };
    let rows = match stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0)) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to query stale code IDs: {}", e);
            return Vec::new();
        }
    };
    rows.flatten().collect()
}
