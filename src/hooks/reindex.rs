//! Hook-triggered background reindex. Spawns a detached `cogz
//! reindex-bg` process so hooks return immediately without blocking
//! on reindex. Includes a debounce mechanism to avoid redundant
//! reindexes when multiple hooks fire in quick succession.

use std::path::{Path, PathBuf};

/// Debounce window in seconds. If a background reindex was triggered
/// within this window, subsequent triggers are skipped.
const DEBOUNCE_SECONDS: i64 = 60;

/// Spawn a background reindex process for the given repo. The hook
/// returns immediately; the reindex runs in a detached process.
///
/// For `session_start`, `force` should be true (no debounce — this
/// is the primary recovery path and only fires once per session).
/// For `prompt_submit`, `force` should be false (debounced — may
/// fire many times per session).
pub fn spawn_reindex_bg(repo: &Path, force: bool) {
    if !force && is_debounced(repo) {
        tracing::debug!("reindex-bg: debounced, skipping");
        return;
    }

    let db_path = match resolve_db_path(repo) {
        Some(p) => p,
        None => {
            tracing::debug!("reindex-bg: no DB found, skipping");
            return;
        }
    };

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cogz"));

    let result = std::process::Command::new(&exe)
        .arg("reindex-bg")
        .arg("--repo")
        .arg(repo)
        .arg("--db")
        .arg(&db_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(child) => {
            tracing::info!(
                "reindex-bg: spawned background reindex (pid={})",
                child.id()
            );
            // Detach the child so it survives the parent's exit.
            drop(child);
            // Record the debounce timestamp.
            let _ = touch_debounce_file(repo);
        }
        Err(e) => {
            tracing::warn!("reindex-bg: failed to spawn: {}", e);
        }
    }
}

/// Check whether a background reindex was triggered recently (within
/// the debounce window). Returns true if debounced (should skip).
fn is_debounced(repo: &Path) -> bool {
    let path = debounce_file(repo);
    let Some(metadata) = std::fs::metadata(&path).ok() else {
        return false;
    };
    let Some(modified) = metadata.modified().ok() else {
        return false;
    };
    let modified: chrono::DateTime<chrono::Utc> = modified.into();
    let elapsed = chrono::Utc::now().signed_duration_since(modified);
    elapsed.num_seconds() < DEBOUNCE_SECONDS
}

/// Write the debounce timestamp file.
fn touch_debounce_file(repo: &Path) -> std::io::Result<()> {
    let path = debounce_file(repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, b"")
}

/// Path for the debounce marker file. Uses a hash of the repo path
/// to avoid collisions when multiple repos are open simultaneously.
fn debounce_file(repo: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    repo.hash(&mut hasher);
    let hash = hasher.finish();
    std::env::temp_dir().join(format!("cogz-reindex-debounce-{}.txt", hash))
}

/// Resolve the DB path for a repo by reading the config. Returns
/// None if config or DB doesn't exist (e.g. no `.cogz/` initialized).
fn resolve_db_path(repo: &Path) -> Option<PathBuf> {
    let cogz_dir = repo.join(".cogz");
    let config_path = cogz_dir.join("config.toml");
    if !config_path.exists() {
        return None;
    }
    let config = crate::config::load(&config_path).ok()?;
    let db_path = crate::config::resolve_db_path(repo, &config.storage.db_path).ok()?;
    if !db_path.exists() {
        return None;
    }
    Some(db_path)
}
