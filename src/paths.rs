//! Per-user filesystem locations shared across modules.

use std::path::PathBuf;

/// Per-user data root: `~/.local/share/cogz` on Unix,
/// `%LOCALAPPDATA%\cogz` on Windows. `None` when no home/app-data
/// directory is resolvable.
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join(".local/share/cogz"));
        }
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            return Some(PathBuf::from(appdata).join("cogz"));
        }
    }
    None
}

/// Transient per-user state: debounce markers, background task ID
/// files. Shared `/tmp` is world-readable and lets one user's stale
/// marker shadow another's — state here is private to the account.
/// Falls back to the shared temp dir only when no per-user data dir
/// exists (e.g. a bare container with no HOME).
pub fn state_dir() -> PathBuf {
    data_dir()
        .map(|d| d.join("state"))
        .unwrap_or_else(std::env::temp_dir)
}
