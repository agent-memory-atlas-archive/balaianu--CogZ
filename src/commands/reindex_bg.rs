//! Background reindex process — spawned by hooks to catch changes
//! from non-hook events (branch switches, pulls, merges, human
//! edits). Runs `reindex_code` (git diff + processing + baseline
//! update), flags stale knowledge, and defers embedding to
//! `embed-bg`.
//!
//! This is the recovery path that `file_save`'s single-file reindex
//! doesn't cover. It runs in a detached process so hooks return
//! immediately without blocking on reindex.

use std::path::{Path, PathBuf};

use cogz::config;
use cogz::index;
use cogz::storage::Storage;

/// Run the background reindex process. Opens the DB, syncs `.cogz/`
/// entity files, runs `reindex_code` (git diff + processing + baseline
/// update), flags stale knowledge, and spawns `embed-bg` for synced
/// entities.
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

    // Flag stale knowledge for changed/deleted code entities.
    let mut all_changed = result.changed_code_ids;
    all_changed.extend(result.deleted_code_ids);
    if !all_changed.is_empty() {
        let flagged =
            index::stale_flagging::flag_stale_knowledge(&storage, &cogz_dir, &all_changed);
        if flagged > 0 {
            eprintln!("reindex-bg: {} knowledge entities flagged stale", flagged);
        }
    }

    // Defer embedding to the background process. Include both file-synced
    // and code-synced entity IDs.
    let mut all_synced = file_result.synced_entity_ids;
    all_synced.extend(result.synced_entity_ids);
    if !all_synced.is_empty() {
        spawn_embed_bg(&storage, &config, &all_synced);
    }

    Ok(())
}

/// Spawn the background embedding process for synced entities.
/// Reuses the same pattern as `cli_embed::spawn_code_embed_background`.
fn spawn_embed_bg(storage: &Storage, config: &config::Config, ids: &[String]) {
    use std::io::Write;

    let id_file = std::env::temp_dir().join(format!(
        "cogz-embed-bg-{}-{}.txt",
        std::process::id(),
        chrono::Utc::now().timestamp()
    ));

    let mut file = match std::fs::File::create(&id_file) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("failed to write background embed ID file: {}", e);
            return;
        }
    };
    if file.write_all(ids.join("\n").as_bytes()).is_err() {
        tracing::warn!("failed to write background embed IDs");
        let _ = std::fs::remove_file(&id_file);
        return;
    }

    let db_path: Option<String> = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT file FROM pragma_database_list() WHERE name='main'",
            [],
            |row| row.get(0),
        )
        .ok()
    };

    let Some(db_path) = db_path else {
        tracing::warn!("failed to get DB path for background embedding");
        let _ = std::fs::remove_file(&id_file);
        return;
    };

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cogz"));

    let result = std::process::Command::new(&exe)
        .arg("embed-bg")
        .arg("--db")
        .arg(&db_path)
        .arg("--ids-file")
        .arg(&id_file)
        .arg("--code-model")
        .arg(&config.embedding.code_model)
        .arg("--dimension")
        .arg(config.embedding.dimension.to_string())
        .arg("--idle-ttl")
        .arg(config.embedding.model_idle_ttl.to_string())
        .arg("--min-free-mb")
        .arg(config.embedding.model_min_free_mb.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(child) => {
            eprintln!(
                "reindex-bg: embedding deferred to background ({} entities, pid={})",
                ids.len(),
                child.id()
            );
            drop(child);
        }
        Err(e) => {
            tracing::warn!("failed to spawn background embedding: {}", e);
            let _ = std::fs::remove_file(&id_file);
        }
    }
}
