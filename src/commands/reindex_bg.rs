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

    // One background worker per repo — a second spawn exits instead
    // of redoing the same indexing work.
    let _bg_lock = match cogz::bg_lock::try_acquire(&cogz_dir) {
        Some(lock) => lock,
        None => {
            eprintln!("reindex-bg: another background worker holds .bg.lock, exiting");
            return Ok(());
        }
    };

    let config = config::load(&config_path)?;
    cogz::embed::set_fts_only(config.embedding.force_fts_only);
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
        "reindex-bg: {} created, {} updated, {} stale, {} skipped, {} hash-migrated",
        result.created, result.updated, result.marked_stale, result.skipped, result.hash_migrated
    );

    // Mine co-change rows for new commits — same channel `index` and
    // `reindex` refresh; without it the feature decays on hook-only
    // repos.
    let cochange_rows = index::cochange::sync_cochange(&storage, repo)?;
    if cochange_rows > 0 {
        eprintln!("reindex-bg: co-change index: {} rows", cochange_rows);
    }

    // Post-index maintenance: flag orphaned knowledge, backfill
    // provenance, recompute drift, heal recovered orphans.
    let post = index::drift::post_index_pass(&storage, &cogz_dir);
    if post.stale_flagged > 0 || post.healed > 0 || post.drifted_entities > 0 {
        eprintln!(
            "reindex-bg: {} flagged stale, {} drifted, {} healed",
            post.stale_flagged, post.drifted_entities, post.healed
        );
    }

    // Embed synced + missing-vector entities through embed_synced —
    // it partitions by entity type: file entities take the knowledge
    // model inline, code goes inline under 50 / embed-bg above. Passing
    // file-backed ids to embed-bg directly would store code-model
    // vectors in the knowledge table — an incompatible vector space.
    let file_targets = super::embed_targets(
        &storage,
        &file_result.synced_entity_ids,
        cogz::storage::embeddings::EmbeddingSpace::Knowledge,
    );
    let code_targets = super::embed_targets(
        &storage,
        &result.synced_entity_ids,
        cogz::storage::embeddings::EmbeddingSpace::Code,
    );
    let mut targets = file_targets;
    targets.extend(code_targets);
    if !targets.is_empty() {
        // `.bg.lock` is already held by this process — an embed-bg
        // spawn defers to it as the designated worker.
        let embedded = crate::cli_embed::embed_synced(&storage, &config, &targets, &cogz_dir, true);
        if embedded > 0 {
            eprintln!("reindex-bg: {} embedded", embedded);
        }
    }

    Ok(())
}
