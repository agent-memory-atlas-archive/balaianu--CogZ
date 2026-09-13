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
    /// (rel_path, source, language) for incremental edge sync.
    pub source_files_for_edges: Vec<(PathBuf, String, Language)>,
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
pub fn parse_source_files(repo_root: &Path, rel_paths: &[PathBuf]) -> ParsedFiles {
    let mut entities_by_file: Vec<(String, Vec<CodeEntity>)> = Vec::new();
    let mut raw_edges_by_file: Vec<(String, Vec<RawEdge>)> = Vec::new();
    let mut source_files_for_edges: Vec<(PathBuf, String, Language)> = Vec::new();
    let mut failed_paths: HashSet<String> = HashSet::new();
    let mut had_read_failures = false;

    for rel_path in rel_paths {
        let abs_path = repo_root.join(rel_path);
        let source = match std::fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("failed to read {}: {}", abs_path.display(), e);
                failed_paths.insert(path_to_string(rel_path));
                had_read_failures = true;
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
        source_files_for_edges.push((rel_path.clone(), source, language));
    }

    ParsedFiles {
        entities_by_file,
        raw_edges_by_file,
        source_files_for_edges,
        failed_paths,
        had_read_failures,
    }
}
