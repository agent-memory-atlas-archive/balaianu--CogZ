//! Background reindex process — spawned by hooks to catch changes
//! from non-hook events (branch switches, pulls, merges, human
//! edits). Runs `reindex_code` (git diff + processing + baseline
//! update), flags stale knowledge, and defers embedding to
//! `embed-bg`.
//!
//! This is the recovery path that `file_save`'s single-file reindex
//! doesn't cover. It runs in a detached process so hooks return
//! immediately without blocking on reindex.

use std::path::Path;

use cogz::config;
use cogz::index;
use cogz::storage::Storage;

/// Run the background reindex process. Opens the DB, syncs `.cogz/`
/// entity files, runs `reindex_code` (git diff + processing + baseline
/// update), flags stale knowledge, and defers embedding to `embed-bg`.
pub fn run_reindex_bg(repo: &Path, db_path: &Path) -> anyhow::Result<()> {
    let cogz_dir = repo.join(".cogz");
    let config_path = cogz_dir.join("config.toml");

    let config = config::load(&config_path)?;
    let storage = Storage::open(db_path, config.embedding.dimension)?;

    // Sync .cogz/ entity files (knowledge, rules, observations) that
    // may have changed outside of hooks (human edits, git pull).
    let file_result = cogz::files::sync_incremental(&storage, &cogz_dir);
    if file_result.created > 0 || file_result.updated > 0 || file_result.marked_stale > 0 {
        eprintln!(
            "reindex-bg: files synced — {} created, {} updated, {} stale",
            file_result.created, file_result.updated, file_result.marked_stale
        );
    }

    // Run the incremental code reindex (git diff + processing + baseline).
    let result = index::reindex_code(&storage, repo, &config);

    eprintln!(
        "reindex-bg: {} created, {} updated, {} stale, {} skipped",
        result.created, result.updated, result.marked_stale, result.skipped
    );

    // Post-index maintenance: flag orphaned knowledge, backfill
    // provenance, recompute drift, heal recovered orphans.
    let post = index::drift::post_index_pass(&storage, &cogz_dir);
    if post.stale_flagged > 0 || post.healed > 0 || post.drifted_entities > 0 {
        eprintln!(
            "reindex-bg: {} flagged stale, {} drifted, {} healed",
            post.stale_flagged, post.drifted_entities, post.healed
        );
    }

    // Defer embedding to the background process. Include both file-synced
    // and code-synced entity IDs.
    let mut all_synced = file_result.synced_entity_ids;
    all_synced.extend(result.synced_entity_ids);
    if !all_synced.is_empty() {
        crate::cli_embed::spawn_code_embed_background(&storage, &config, &all_synced);
    }

    Ok(())
}
