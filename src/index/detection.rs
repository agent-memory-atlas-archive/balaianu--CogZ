//! Change detection — unified interface for finding files that changed
//! since the last index.
//!
//! Currently only git-based detection is implemented. The interface is
//! designed so a no-git fallback (mtime manifest) can be added later
//! without changing callers.

use std::path::{Path, PathBuf};

pub use crate::index::git_diff::{ChangeType, ChangedFile};

/// Detect changed source files since the baseline commit.
///
/// Tries git diff first. Returns `None` if git is unavailable, the
/// repo has no commits, or no baseline is stored — the caller should
/// do a full scan in that case.
pub fn detect_changed_files(
    repo_root: &Path,
    baseline_sha: Option<&str>,
) -> Option<Vec<ChangedFile>> {
    crate::index::git_diff::changed_source_files(repo_root, baseline_sha)
}

/// Get the current HEAD commit SHA, or `None` if not a git repo or
/// no commits exist.
pub fn head_sha(repo_root: &Path) -> Option<String> {
    crate::index::git_diff::head_sha(repo_root)
}

/// Extract the paths from a list of changed files.
pub fn changed_paths(changed: &[ChangedFile]) -> Vec<PathBuf> {
    changed.iter().map(|f| f.path.clone()).collect()
}
