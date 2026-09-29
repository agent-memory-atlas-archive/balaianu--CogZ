//! Git-history co-change mining — which commit-message terms travel
//! with which *entities*. Derived index state, rebuilt from
//! `git log -p -U0` on every `cogz index`; never written to
//! canonical files.
//!
//! A commit message describes intent in prose ("deprecate the app=
//! shortcut"); the entities its hunks intersect are where that intent
//! lives. Repeated across history, the pairing becomes a vocabulary
//! bridge no embedding supplies: query terms map to entities that
//! changed under those terms before. Rows keep the commit id and
//! timestamp so a query can bound the history it draws on
//! (`SearchParams::before_ts`) — which is also how the benchmark
//! keeps itself honest.
//!
//! Hunk ranges are mapped against *current* entity positions, so
//! very old commits associate with whatever occupies their lines now
//! — the same approximation the benchmark's own ground truth uses.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::search::prf;
use crate::storage::{Storage, StorageError};

/// Cap on commits walked — recent history aligns with current entity
/// positions anyway (line drift grows with age).
const MAX_COMMITS: usize = 20_000;

/// \x1f separates commit-header fields; it cannot appear inside a
/// diff line, so it never collides with hunk or file markers.
const HDR: char = '\u{1f}';

struct Commit {
    sha: String,
    ts: i64,
    terms: HashSet<String>,
    /// file_path → new-side hunk ranges this commit touched
    files: HashMap<String, Vec<(i64, i64)>>,
}

/// Rebuild the `cochange` table. Returns rows written. Zero on
/// non-git repos or empty history — the search channel simply
/// contributes nothing.
pub fn sync_cochange(storage: &Storage, repo: &Path) -> Result<usize, StorageError> {
    let child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "log",
            "-p",
            "-U0",
            "--no-merges",
            "--format=%x1f%H%x1f%ct%x1f%s",
            "-n",
            &MAX_COMMITS.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return Ok(0) };
    let Some(stdout) = child.stdout.take() else {
        return Ok(0);
    };

    let mut commits: Vec<Commit> = Vec::new();
    let mut cur: Option<Commit> = None;
    let mut cur_file: Option<String> = None;

    // Byte-split + lossy decode: binary patches would abort a strict
    // UTF-8 `lines()` iterator mid-history.
    for raw in BufReader::new(stdout).split(b'\n') {
        let Ok(raw) = raw else { break };
        let line = String::from_utf8_lossy(&raw);
        if let Some(rest) = line.strip_prefix(HDR) {
            if let Some(c) = cur.take() {
                commits.push(c);
            }
            let mut it = rest.splitn(3, HDR);
            let sha = it.next().unwrap_or_default().to_string();
            let ts: i64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let terms = prf::content_terms(it.next().unwrap_or_default())
                .into_iter()
                .collect();
            cur = Some(Commit {
                sha,
                ts,
                terms,
                files: HashMap::new(),
            });
            cur_file = None;
        } else if let Some(path) = line.strip_prefix("+++ b/") {
            cur_file = Some(path.trim().to_string());
        } else if line.starts_with("+++ ") {
            // `+++ /dev/null` (deleted file) or quoted path — no entity
            // can match it at HEAD positions.
            cur_file = None;
        } else if line.starts_with("@@ ")
            && let Some((s, e)) = parse_hunk_range(&line)
            && let Some(c) = &mut cur
            && let Some(fp) = &cur_file
        {
            c.files.entry(fp.clone()).or_default().push((s, e));
        }
    }
    if let Some(c) = cur.take() {
        commits.push(c);
    }
    let git_status = child.wait();
    if commits.is_empty() {
        return Ok(0);
    }
    // A mid-stream failure yields silently truncated history — the
    // rows that follow are still valid but incomplete, so warn.
    if !matches!(git_status, Ok(s) if s.success()) {
        tracing::warn!("git log exited early; co-change history is truncated");
    }

    // Entity lookup per file: (id, line_start, line_end), cached —
    // a file touched by 500 commits is queried once. One transaction
    // covers delete, lookup, and insert so the table never sits empty
    // for a concurrent reader mid-rebuild.
    let files_needed: HashSet<&String> = commits.iter().flat_map(|c| c.files.keys()).collect();
    let ent_stmt = "SELECT id, json_extract(properties, '$.line_start'), \
                json_extract(properties, '$.line_end') \
         FROM entities \
         WHERE file_path = ? AND type IN ('function', 'class') \
         AND status = 'active'";

    let mut entities_by_file: HashMap<String, Vec<(String, i64, i64)>> = HashMap::new();
    let mut conn = storage.conn();
    let tx = conn.transaction()?;
    let mut n = 0usize;
    {
        tx.execute("DELETE FROM cochange", [])?;
        let mut es = tx.prepare(ent_stmt)?;
        for fp in files_needed {
            let rows = es
                .query_map([fp.as_str()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            entities_by_file.insert(
                fp.clone(),
                rows.into_iter()
                    .filter_map(|(id, ls, le)| Some((id, ls?, le?)))
                    .collect(),
            );
        }
        drop(es);

        // Intersect each commit's hunks with its files' entities.
        let mut ins = tx.prepare(
            "INSERT OR REPLACE INTO cochange (term, entity_id, commit_id, commit_ts) \
             VALUES (?, ?, ?, ?)",
        )?;
        for c in &commits {
            if c.terms.is_empty() {
                continue;
            }
            let mut touched: HashSet<&String> = HashSet::new();
            for (fp, ranges) in &c.files {
                let Some(ents) = entities_by_file.get(fp) else {
                    continue;
                };
                for (id, ls, le) in ents {
                    if ranges.iter().any(|(hs, he)| ls <= he && le >= hs) {
                        touched.insert(id);
                    }
                }
            }
            for t in &c.terms {
                for e in &touched {
                    ins.execute(rusqlite::params![t, e, c.sha, c.ts])?;
                    n += 1;
                }
            }
        }
    }
    tx.commit()?;
    Ok(n)
}

/// Parse `@@ -a(,b)? +c(,d)? @@` into the new-side range (start, end)
/// inclusive. A zero-length span (+c,0) collapses to the point (c, c)
/// — matching the benchmark ground truth's range convention.
fn parse_hunk_range(line: &str) -> Option<(i64, i64)> {
    let plus = line.find('+')?;
    let after = &line[plus + 1..];
    let end = after.find(' ').or_else(|| after.find('@'))?;
    let spec = &after[..end];
    let (start, span) = if let Some((s, d)) = spec.split_once(',') {
        (s.parse().ok()?, d.parse().ok()?)
    } else {
        (spec.parse().ok()?, 1i64)
    };
    Some((start, start + (span - 1).max(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn insert_fn_entity(storage: &Storage, id: &str, fp: &str, ls: i64, le: i64) {
        let conn = storage.conn();
        let mut e = crate::storage::crud::Entity::new(id, "function", "auth_fn", "fn auth");
        e.properties = serde_json::json!({"file_path": fp, "line_start": ls, "line_end": le});
        e.file_path = Some(fp.to_string());
        crate::storage::crud::insert_entity(&conn, &e).unwrap();
    }

    #[test]
    fn parse_hunk_range_variants() {
        assert_eq!(parse_hunk_range("@@ -0,0 +1,3 @@ fn"), Some((1, 3)));
        assert_eq!(parse_hunk_range("@@ -5 +9 @@"), Some((9, 9)));
        assert_eq!(parse_hunk_range("@@ -2,4 +10,0 @@"), Some((10, 10)));
        assert_eq!(parse_hunk_range("not a hunk"), None);
    }

    #[test]
    fn sync_cochange_maps_terms_to_touched_entities() {
        let dir = std::env::temp_dir().join(format!("cogz_co_t_{}", std::process::id()));
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let sh = |args: &[&str], cdate: &str| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .env("GIT_COMMITTER_DATE", cdate)
                .output()
                .unwrap();
        };
        sh(&["init", "-q"], "");
        sh(&["config", "user.email", "t@t"], "");
        sh(&["config", "user.name", "t"], "");
        // Commit 1: adds a.py (entity lines 1-2), term "authentication"
        fs::write(repo.join("a.py"), "def auth():\n    pass\n").unwrap();
        sh(&["add", "."], "");
        sh(
            &["commit", "-q", "-m", "Add authentication flow"],
            "2020-01-01T00:00:00Z",
        );
        // Commit 2: adds b.py, term "deprecate"
        fs::write(repo.join("b.py"), "def other():\n    pass\n").unwrap();
        sh(&["add", "."], "");
        sh(
            &["commit", "-q", "-m", "Deprecate auth token"],
            "2020-06-01T00:00:00Z",
        );

        let storage = Storage::open(&dir.join("state.db"), 384).unwrap();
        insert_fn_entity(&storage, "ent-a", "a.py", 1, 2);
        insert_fn_entity(&storage, "ent-b", "b.py", 1, 2);

        let n = sync_cochange(&storage, &repo).unwrap();
        assert!(n > 0);
        let conn = storage.conn();
        let rows: Vec<(String, String, i64)> = conn
            .prepare("SELECT term, entity_id, commit_ts FROM cochange ORDER BY term")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            rows.iter()
                .any(|(t, e, _)| t == "authentication" && e == "ent-a")
        );
        assert!(
            rows.iter()
                .any(|(t, e, _)| t == "deprecate" && e == "ent-b")
        );
        // No cross-wiring: "authentication" must not map to ent-b.
        assert!(
            !rows
                .iter()
                .any(|(t, e, _)| t == "authentication" && e == "ent-b")
        );
        let ts_a: i64 = rows.iter().find(|(_, e, _)| e == "ent-a").unwrap().2;
        let ts_b: i64 = rows.iter().find(|(_, e, _)| e == "ent-b").unwrap().2;
        assert!(ts_b > ts_a);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sync_cochange_non_git_returns_zero() {
        let dir = std::env::temp_dir().join(format!("cogz_co_n_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let storage = Storage::open(&dir.join("state.db"), 384).unwrap();
        let n = sync_cochange(&storage, &dir).unwrap();
        assert_eq!(n, 0);
        fs::remove_dir_all(&dir).ok();
    }
}
