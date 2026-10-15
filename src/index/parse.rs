//! Source file parsing — shared between full scan and incremental reindex.
//!
//! Reads source files, detects language, and runs tree-sitter extraction
//! to produce entities and raw edges. No lock held — pure CPU + I/O.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::index::gitignore;
use crate::index::path_to_string;
use crate::index::tree_sitter::{self, CodeEntity, Language, RawEdge};

/// Result of parsing a set of source files.
pub struct ParsedFiles {
    /// (relative_path, entities) for entity sync.
    pub entities_by_file: Vec<(String, Vec<CodeEntity>)>,
    /// (relative_path, raw_edges) for full edge sync.
    pub raw_edges_by_file: Vec<(String, Vec<RawEdge>)>,
    /// Paths that failed to read or parse. Excluded from stale marking
    /// in full scan mode — a read failure does not mean the file was
    /// deleted, and marking its entities stale would lose valid edges.
    pub failed_paths: HashSet<String>,
    /// Whether any file failed to read (I/O error). Used by incremental
    /// reindex to decide whether to advance the baseline commit.
    /// Language detection failures do not set this — they are permanent
    /// (unsupported syntax), not transient (retryable).
    pub had_read_failures: bool,
}

/// Parse a list of source files. Reads each file, detects language,
/// and runs tree-sitter extraction in a single pass.
///
/// Both full scan and incremental reindex call this. Callers decide
/// which output fields to use based on their sync mode.
///
/// Files larger than `max_bytes` are skipped by size before reading —
/// a minified bundle or generated file should never load into memory.
/// They land in `failed_paths` (no stale marking) but are permanent
/// skips, so they do not pin the incremental baseline.
pub fn parse_source_files(repo_root: &Path, rel_paths: &[PathBuf], max_bytes: u64) -> ParsedFiles {
    let mut entities_by_file: Vec<(String, Vec<CodeEntity>)> = Vec::new();
    let mut raw_edges_by_file: Vec<(String, Vec<RawEdge>)> = Vec::new();
    let mut failed_paths: HashSet<String> = HashSet::new();
    let mut had_read_failures = false;

    for rel_path in rel_paths {
        let abs_path = repo_root.join(rel_path);
        if let Ok(meta) = std::fs::metadata(&abs_path)
            && meta.len() > max_bytes
        {
            tracing::warn!(
                "skipping {}: {} bytes exceeds index.max_file_bytes ({})",
                abs_path.display(),
                meta.len(),
                max_bytes
            );
            failed_paths.insert(path_to_string(rel_path));
            continue;
        }
        let source = match std::fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("failed to read {}: {}", abs_path.display(), e);
                failed_paths.insert(path_to_string(rel_path));
                // Permanent failures — non-UTF-8 content, denied
                // permissions — retry identically every run; pinning
                // the baseline on them freezes incremental reindex
                // forever. Transient errors (lock contention, races
                // with a writer) still pin.
                had_read_failures |= !matches!(
                    e.kind(),
                    std::io::ErrorKind::InvalidData | std::io::ErrorKind::PermissionDenied
                );
                continue;
            }
        };
        let language = match gitignore::language_for_path(rel_path) {
            Some(lang) => match Language::parse_str(lang) {
                Some(l) => l,
                None => {
                    failed_paths.insert(path_to_string(rel_path));
                    continue;
                }
            },
            None => {
                failed_paths.insert(path_to_string(rel_path));
                continue;
            }
        };

        let (entities, raw_edges) = tree_sitter::extract_all(rel_path, &source, language);
        let path_str = path_to_string(rel_path);
        entities_by_file.push((path_str.clone(), entities));
        raw_edges_by_file.push((path_str, raw_edges));
    }

    ParsedFiles {
        entities_by_file,
        raw_edges_by_file,
        failed_paths,
        had_read_failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_read_errors_do_not_pin_baseline() {
        // A non-UTF-8 .py retries identically forever — it must land
        // in failed_paths (stale-marking protection) without setting
        // had_read_failures, or incremental reindex never advances.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.py"), b"\xff\xfe\x00bad").unwrap();
        std::fs::write(dir.path().join("ok.py"), "def f():\n    pass\n").unwrap();

        let parsed = parse_source_files(
            dir.path(),
            &[PathBuf::from("bad.py"), PathBuf::from("ok.py")],
            u64::MAX,
        );

        assert!(parsed.failed_paths.contains("bad.py"));
        assert!(
            !parsed.had_read_failures,
            "permanent read error must not pin the baseline"
        );
        assert_eq!(parsed.entities_by_file.len(), 1);
    }

    #[test]
    fn oversized_file_is_a_permanent_skip() {
        // An over-cap file must land in failed_paths (its entities are
        // not stale-marked) without setting had_read_failures — the
        // skip is permanent, so pinning the baseline would freeze
        // incremental reindex forever.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("big.py"), "# padding\n".repeat(1024)).unwrap();
        std::fs::write(dir.path().join("ok.py"), "def f():\n    pass\n").unwrap();

        let parsed = parse_source_files(
            dir.path(),
            &[PathBuf::from("big.py"), PathBuf::from("ok.py")],
            1024,
        );

        assert!(parsed.failed_paths.contains("big.py"));
        assert!(!parsed.had_read_failures);
        assert_eq!(parsed.entities_by_file.len(), 1);
    }
}
