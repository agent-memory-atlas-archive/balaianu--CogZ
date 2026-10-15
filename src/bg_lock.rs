//! Single background worker per repository.
//!
//! `reindex-bg` and `embed-bg` are spawned detached — two events in
//! quick succession can start overlapping workers that redo the same
//! indexing and embedding work. Every background worker takes
//! `.cogz/.bg.lock` with `try_lock_exclusive` and exits when it is
//! already held.

use std::fs::File;
use std::path::Path;

/// Try to become the one background worker for this repo. Returns the
/// open lock file — the flock lives as long as the `File` — or `None`
/// when another worker holds the lock or `.cogz/` is missing.
pub fn try_acquire(cogz_dir: &Path) -> Option<File> {
    use fs2::FileExt;
    let lock_path = cogz_dir.join(".bg.lock");
    let file = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock_path)
        .ok()?;
    file.try_lock_exclusive().ok()?;
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_worker_is_refused_until_first_drops() {
        let dir = tempfile::tempdir().unwrap();
        let cogz_dir = dir.path().join(".cogz");
        std::fs::create_dir_all(&cogz_dir).unwrap();

        let first = try_acquire(&cogz_dir).expect("first worker takes the lock");
        // flock conflicts across open file descriptions, same process
        // included — this is the real double-worker condition.
        assert!(try_acquire(&cogz_dir).is_none());
        drop(first);
        assert!(try_acquire(&cogz_dir).is_some());
    }

    #[test]
    fn missing_cogz_dir_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(try_acquire(&dir.path().join(".cogz")).is_none());
    }
}
