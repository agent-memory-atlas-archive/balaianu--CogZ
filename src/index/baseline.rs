//! Baseline commit management — read, write, and decide whether to
//! advance the baseline after an incremental reindex.
//!
//! The baseline commit is stored in the `meta` table as
//! `last_indexed_commit`. It marks the last commit that was fully
//! indexed. Incremental reindex diffs the baseline tree against the
//! working directory to find changed files.

use std::path::Path;

use crate::index::detection;
use crate::storage::{self, Storage};

/// Meta key for the baseline commit SHA.
const BASELINE_KEY: &str = "last_indexed_commit";

/// Read the stored baseline commit SHA, or `None` if no baseline exists.
pub fn read_baseline(storage: &Storage) -> Option<String> {
    let conn = storage.conn();
    storage::get_meta(&conn, BASELINE_KEY)
}

/// Update the baseline to the current HEAD commit. Logs a warning on
/// failure — the baseline is best-effort, not a correctness invariant.
pub fn update_baseline(storage: &Storage, repo_root: &Path) {
    if let Some(sha) = detection::head_sha(repo_root) {
        let conn = storage.conn();
        if let Err(e) = storage::set_meta(&conn, BASELINE_KEY, &sha) {
            tracing::warn!("failed to record {}: {}", BASELINE_KEY, e);
        }
    }
}

/// Whether the baseline should advance after an incremental reindex.
///
/// Only advance when all changed files were successfully read. On
/// partial failure, preserve the previous baseline so the next
/// incremental reindex retries the failed files.
pub fn should_update_baseline(had_read_failures: bool) -> bool {
    !had_read_failures
}
