//! Git provenance for injected rules — last commit and author per
//! file, collected in one bounded `git log --name-only` pass.
//!
//! Rules are behavioral constraints injected into agent context;
//! knowing who last changed one (and in which commit) lets the
//! reader weigh it like code. Missing git or history yields an
//! empty map — provenance is enrichment, never a delivery gate.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

/// How far back one provenance pass may walk. Enough to find the
/// last touch of every rule file in any realistic repo; beyond that
/// the rule is effectively unreviewed history anyway.
const MAX_COMMITS: &str = "1000";

/// Last-commit metadata for one tracked file.
#[derive(Debug, Clone)]
pub struct Provenance {
    /// Abbreviated commit hash.
    pub commit: String,
    /// Author name.
    pub author: String,
    /// Commit date, `YYYY-MM-DD`.
    pub date: String,
}

/// Map each repo-relative path to the commit that last touched it.
/// `git log` runs newest-first, so the first occurrence of a path in
/// the output is its most recent change.
pub fn collect(repo_root: &Path, paths: &[String]) -> HashMap<String, Provenance> {
    if paths.is_empty() {
        return HashMap::new();
    }
    let mut args: Vec<String> = vec![
        "-C".into(),
        repo_root.to_string_lossy().into_owned(),
        "log".into(),
        "--no-merges".into(),
        "--format=%H%x1f%an%x1f%ad".into(),
        "--date=short".into(),
        "--name-only".into(),
        "-n".into(),
        MAX_COMMITS.into(),
        "--".into(),
    ];
    args.extend(paths.iter().cloned());
    let Ok(out) = Command::new("git").args(&args).output() else {
        return HashMap::new();
    };
    if !out.status.success() {
        return HashMap::new();
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let wanted: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let mut map = HashMap::new();
    let mut current: Option<Provenance> = None;

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some((hash, rest)) = line.split_once('\x1f') {
            let (author, date) = rest.split_once('\x1f').unwrap_or((rest, ""));
            current = Some(Provenance {
                commit: hash.chars().take(12).collect(),
                author: author.to_string(),
                date: date.to_string(),
            });
        } else if let Some(prov) = &current
            && wanted.contains(line)
            && !map.contains_key(line)
        {
            map.insert(line.to_string(), prov.clone());
            if map.len() == wanted.len() {
                break;
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two commits over distinct files: provenance must attribute each
    /// path to the commit that last touched it, not the latest commit.
    #[test]
    fn collect_attributes_last_touch_per_path() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Tester", "t@example.com").unwrap();
        let mut index = repo.index().unwrap();
        for (file, content) in [("a.md", "one"), ("b.md", "two")] {
            std::fs::write(dir.path().join(file), content).unwrap();
            index.add_path(Path::new(file)).unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let parents: Vec<git2::Commit> = repo
                .head()
                .ok()
                .and_then(|h| h.peel_to_commit().ok())
                .into_iter()
                .collect();
            let refs: Vec<&git2::Commit> = parents.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &refs)
                .unwrap();
        }
        std::fs::write(dir.path().join("a.md"), "one again").unwrap();
        index.add_path(Path::new("a.md")).unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "c2", &tree, &[&parent])
            .unwrap();

        let paths = vec!["a.md".to_string(), "b.md".to_string()];
        let map = collect(dir.path(), &paths);
        assert_eq!(map.len(), 2);
        assert_eq!(map["a.md"].author, "Tester");
        assert_ne!(map["a.md"].commit, map["b.md"].commit);

        let missing = collect(dir.path(), &["nope.md".to_string()]);
        assert!(missing.is_empty());
    }
}
