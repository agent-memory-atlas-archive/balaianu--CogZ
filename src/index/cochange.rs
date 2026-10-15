//! Git-history co-change mining — which commit-message terms travel
//! with which *entities*. Derived index state, rebuilt from commit
//! diffs (git2, `-U0` semantics) on every `cogz index`; never written
//! to canonical files. In-process libgit2 diffs keep this working
//! where no `git` binary exists and immune to user config
//! (`textconv`, external diff drivers) that would corrupt `-p` output.
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
use std::path::Path;

use crate::search::prf;
use crate::storage::{Storage, StorageError};

/// Cap on commits walked — recent history aligns with current entity
/// positions anyway (line drift grows with age).
const MAX_COMMITS: usize = 20_000;

/// Commits flushed to the staging table at a time — bounds the
/// parse buffer so 20k-commit histories don't live in memory.
const STAGE_BATCH: usize = 64;

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
    let Ok(git) = git2::Repository::discover(repo) else {
        return Ok(0);
    };

    // Rows are staged per batch into a connection-local temp table —
    // the stream keeps at most STAGE_BATCH commits in memory instead
    // of buffering all of git history. The final swap keeps the
    // rebuild atomic for readers.
    {
        let conn = storage.conn();
        conn.execute_batch(
            "DROP TABLE IF EXISTS cochange_stage;
             CREATE TEMP TABLE cochange_stage (
                 term      TEXT NOT NULL,
                 entity_id TEXT NOT NULL,
                 commit_id TEXT NOT NULL,
                 commit_ts INTEGER NOT NULL
             );",
        )?;
    }

    let Ok(mut revwalk) = git.revwalk() else {
        return Ok(0);
    };
    let _ = revwalk.set_sorting(git2::Sort::TIME);
    if revwalk.push_head().is_err() {
        return Ok(0);
    }

    let mut pending: Vec<Commit> = Vec::new();
    let mut entities_by_file: HashMap<String, Vec<(String, i64, i64)>> = HashMap::new();
    let mut n = 0usize;
    let mut saw_commit = false;

    for oid in revwalk.take(MAX_COMMITS).flatten() {
        let Ok(commit) = git.find_commit(oid) else {
            continue;
        };
        // --no-merges
        if commit.parent_count() > 1 {
            continue;
        }
        saw_commit = true;
        let mut c = Commit {
            sha: oid.to_string(),
            ts: commit.time().seconds(),
            terms: prf::content_terms(commit.summary().ok().flatten().unwrap_or_default())
                .into_iter()
                .collect(),
            files: HashMap::new(),
        };
        // Terms gate the diff: a message-free commit contributes no
        // rows, so its hunk walk is wasted work.
        if !c.terms.is_empty() {
            collect_diff_files(&git, &commit, &mut c.files);
        }
        pending.push(c);
        if pending.len() >= STAGE_BATCH {
            let conn = storage.conn();
            n += stage_batch(&conn, &pending, &mut entities_by_file)?;
            pending.clear();
        }
    }
    if !saw_commit {
        return Ok(0);
    }

    if !pending.is_empty() {
        let conn = storage.conn();
        n += stage_batch(&conn, &pending, &mut entities_by_file)?;
    }

    // Atomic swap: delete + insert in one transaction so the table
    // never sits empty (or half-new) for a concurrent reader.
    let mut conn = storage.conn();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM cochange", [])?;
    tx.execute(
        "INSERT OR REPLACE INTO cochange (term, entity_id, commit_id, commit_ts) \
         SELECT term, entity_id, commit_id, commit_ts FROM cochange_stage",
        [],
    )?;
    tx.commit()?;
    conn.execute_batch("DROP TABLE IF EXISTS cochange_stage")?;
    Ok(n)
}

/// Diff a commit against its first parent (the empty tree for a root
/// commit) with `-U0` semantics and collect each touched file's
/// new-side hunk ranges. Deleted files contribute nothing — no entity
/// can match them at HEAD positions. Diff failures leave the commit
/// with no file ranges rather than aborting the whole walk.
fn collect_diff_files(
    git: &git2::Repository,
    commit: &git2::Commit,
    files: &mut HashMap<String, Vec<(i64, i64)>>,
) {
    let new_tree = commit.tree().ok();
    let old_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(0);
    let Ok(diff) = git.diff_tree_to_tree(old_tree.as_ref(), new_tree.as_ref(), Some(&mut opts))
    else {
        return;
    };
    let _ = diff.foreach(
        // Skip deleted files outright — no entity can match them.
        &mut |delta, _| delta.status() != git2::Delta::Deleted,
        None,
        Some(&mut |delta, hunk| {
            let Some(path) = delta.new_file().path() else {
                return true;
            };
            let start = i64::from(hunk.new_start());
            let end = start + (hunk.new_lines() as i64 - 1).max(0);
            files
                .entry(path.to_string_lossy().into_owned())
                .or_default()
                .push((start, end));
            true
        }),
        None,
    );
}

/// Intersect one batch of commits' hunks with their files' entities
/// and stage the resulting rows into `cochange_stage`. Entity lookup
/// is per-file and cached across batches — a file touched by 500
/// commits is queried once.
fn stage_batch(
    conn: &rusqlite::Connection,
    batch: &[Commit],
    entities_by_file: &mut HashMap<String, Vec<(String, i64, i64)>>,
) -> Result<usize, StorageError> {
    let ent_stmt = "SELECT id, json_extract(properties, '$.line_start'), \
                json_extract(properties, '$.line_end') \
         FROM entities \
         WHERE file_path = ? AND type IN ('function', 'class') \
         AND status = 'active'";

    let mut es = conn.prepare(ent_stmt)?;
    let mut ins = conn.prepare(
        "INSERT INTO cochange_stage (term, entity_id, commit_id, commit_ts) \
         VALUES (?, ?, ?, ?)",
    )?;

    let mut n = 0usize;
    for c in batch {
        if c.terms.is_empty() {
            continue;
        }
        // Fill the entity cache for this commit's files before
        // borrowing it for the intersect pass.
        for fp in c.files.keys() {
            if entities_by_file.contains_key(fp) {
                continue;
            }
            let rows: Vec<(String, i64, i64)> = es
                .query_map([fp.as_str()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|(id, ls, le)| Some((id, ls?, le?)))
                .collect();
            entities_by_file.insert(fp.clone(), rows);
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
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn insert_fn_entity(storage: &Storage, id: &str, fp: &str, ls: i64, le: i64) {
        let conn = storage.conn();
        let mut e = crate::storage::crud::Entity::new(id, "function", "auth_fn", "fn auth");
        e.properties = serde_json::json!({"file_path": fp, "line_start": ls, "line_end": le});
        e.file_path = Some(fp.to_string());
        crate::storage::crud::insert_entity(&conn, &e).unwrap();
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

    #[test]
    fn sync_cochange_stages_across_batch_boundary() {
        // > STAGE_BATCH commits — rows from both the mid-stream flush
        // and the tail flush must land in cochange.
        let dir = std::env::temp_dir().join(format!("cogz_co_b_{}", std::process::id()));
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let sh = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
        };
        sh(&["init", "-q"]);
        sh(&["config", "user.email", "t@t"]);
        sh(&["config", "user.name", "t"]);

        const COMMITS: usize = 70;
        for i in 0..COMMITS {
            // Flip line 2 each commit so every hunk intersects the
            // entity's 1-2 line range.
            let content = format!("def auth():\n    pass  # {i}\n");
            fs::write(repo.join("a.py"), content).unwrap();
            sh(&["add", "."]);
            let msg = if i == 0 {
                "seed baseline auth".to_string()
            } else if i == COMMITS - 1 {
                "final polish auth".to_string()
            } else {
                format!("tweak iteration {i}")
            };
            sh(&["commit", "-q", "-m", &msg]);
        }

        let storage = Storage::open(&dir.join("state.db"), 384).unwrap();
        insert_fn_entity(&storage, "ent-a", "a.py", 1, 2);

        let n = sync_cochange(&storage, &repo).unwrap();
        assert!(n > 0);
        let conn = storage.conn();
        let terms: std::collections::HashSet<String> = conn
            .prepare("SELECT DISTINCT term FROM cochange")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        // First commit was flushed mid-stream; last was flushed in
        // the tail pass — both terms must survive the swap.
        assert!(terms.contains("seed"), "mid-stream batch rows missing");
        assert!(terms.contains("polish"), "tail batch rows missing");
        fs::remove_dir_all(&dir).ok();
    }
}
