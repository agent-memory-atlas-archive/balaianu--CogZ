//! Health check implementations for `cogz doctor`.

#[path = "checks_analysis.rs"]
mod checks_analysis;

pub(crate) use checks_analysis::{check_corrupt_json, check_near_duplicates, check_vec_dimensions};

use std::path::Path;

use rusqlite::Connection;

use crate::config::Config;
use crate::storage::Storage;

/// A single issue found by the doctor.
#[derive(Debug, Clone)]
pub struct Issue {
    pub kind: IssueKind,
    pub entity_id: Option<String>,
    pub message: String,
}

/// Category of issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// DB entity exists but file is missing on disk.
    MissingFile,
    /// File exists but DB entity is missing (not synced).
    MissingEntity,
    /// DB entity is not marked stale but file hash differs.
    StaleNotFlagged,
    /// Observation content was edited (append-only violation).
    ObservationEdited,
    /// Rule content was substantively changed.
    RuleEdited,
    /// Entity status transitioned illegally.
    IllegalStatusTransition,
    /// Rule is superseded but no derived_from edge to a new rule.
    OrphanedSupersede,
    /// Two knowledge entries have embedding similarity > 0.80.
    NearDuplicate,
    /// vec0 table dimension doesn't match configured embedding dimension.
    DimensionMismatch,
    /// Entity properties or event payload JSON is corrupt.
    CorruptJson,
    /// Embedding blob could not be read or parsed.
    CorruptEmbedding,
    /// `volatility: record` on an entity class that isn't naturally a
    /// record — silently exempts it from all drift/stale tracking.
    VolatilityFootgun,
    /// Knowledge category that resembles a known category without
    /// matching it — likely a typo that changes classification.
    CategoryTypo,
}

impl std::fmt::Display for IssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFile => write!(f, "missing_file"),
            Self::MissingEntity => write!(f, "missing_entity"),
            Self::StaleNotFlagged => write!(f, "stale_not_flagged"),
            Self::ObservationEdited => write!(f, "observation_edited"),
            Self::RuleEdited => write!(f, "rule_edited"),
            Self::IllegalStatusTransition => write!(f, "illegal_status_transition"),
            Self::OrphanedSupersede => write!(f, "orphaned_supersede"),
            Self::NearDuplicate => write!(f, "near_duplicate"),
            Self::DimensionMismatch => write!(f, "dimension_mismatch"),
            Self::CorruptJson => write!(f, "corrupt_json"),
            Self::CorruptEmbedding => write!(f, "corrupt_embedding"),
            Self::VolatilityFootgun => write!(f, "volatility_footgun"),
            Self::CategoryTypo => write!(f, "category_typo"),
        }
    }
}

/// Usage instrumentation summary. `None` when the usage tables don't
/// exist (pre-v4 database) — doctor degrades rather than fails.
#[derive(Debug, Default, Clone)]
pub struct UsageReport {
    pub pack_hits: usize,
    pub pack_misses: usize,
    pub search_hits: usize,
    pub search_misses: usize,
    pub pending: usize,
    /// Entities never surfaced in any delivery within the recent
    /// session window.
    pub dead_weight: Vec<String>,
    /// File-backed entities (observation/rule/knowledge) never
    /// delivered at all.
    pub never_retrieved_files: Vec<String>,
    /// (tier, hits, misses) — resolved outcomes per delivery tier.
    pub tier_rates: Vec<(String, usize, usize)>,
    /// (entity_type, hits, misses) — resolved outcomes per type.
    pub type_rates: Vec<(String, usize, usize)>,
}

/// Full doctor report.
#[derive(Debug, Default)]
pub struct DoctorReport {
    pub issues: Vec<Issue>,
    pub db_healthy: bool,
    pub schema_version: u32,
    pub entity_count: i64,
    pub edge_count: i64,
    pub models_available: bool,
    pub usage: Option<UsageReport>,
}

/// Run all health checks and return a report.
pub fn run_doctor(
    storage: &Storage,
    config: &Config,
    cogz_dir: &Path,
    repo_root: &Path,
) -> DoctorReport {
    let mut report = DoctorReport::default();
    let conn = storage.conn();

    // DB integrity checks
    report.schema_version = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    report.entity_count = crate::storage::crud::count_all(&conn).unwrap_or(0);
    report.edge_count = crate::storage::edges::count_edges(&conn).unwrap_or(0);
    report.db_healthy = check_integrity(&conn);

    // Model availability
    let models_dir = crate::embed::models_dir();
    report.models_available = check_models(config, &models_dir);

    // File sync consistency
    check_file_sync(&conn, cogz_dir, repo_root, &mut report);

    // Policy violations
    check_observation_edits(&conn, cogz_dir, &mut report);
    check_orphaned_supersedes(&conn, &mut report);

    // Near-duplicate knowledge (requires embeddings)
    check_near_duplicates(&conn, &mut report);

    // vec0 dimension mismatch (detects config changes after DB creation)
    check_vec_dimensions(&conn, config, &mut report);

    // Corrupt entity properties or event payloads
    check_corrupt_json(&conn, &mut report);

    // Staleness-tracking hygiene: record overrides on living-class
    // entities, and category spellings that lose record status.
    check_volatility_overrides(&conn, &mut report);
    check_category_typos(&conn, &mut report);

    // Usage instrumentation (hit rate, dead weight, write quality)
    report.usage = check_usage(&conn);

    report
}

/// Sessions a dead-weight entity must have survived to count.
const DEAD_WEIGHT_SESSIONS: usize = 10;

fn check_usage(conn: &Connection) -> Option<UsageReport> {
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type = 'table' AND name IN ('deliveries', 'entity_usage')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if tables != 2 {
        return None;
    }

    use crate::storage::usage;
    let pack = usage::usage_summary(conn, Some(usage::DeliveryKind::Pack)).ok()?;
    let search = usage::usage_summary(conn, Some(usage::DeliveryKind::Search)).ok()?;
    let cutoff = usage::session_cutoff(conn, DEAD_WEIGHT_SESSIONS)
        .ok()
        .flatten();
    let dead_weight = match cutoff {
        Some(ts) => usage::never_delivered_since(conn, &ts).ok()?,
        None => usage::never_delivered(conn).ok()?,
    };
    let never_retrieved_files =
        usage::never_delivered_by_type(conn, &["observation", "rule", "knowledge"]).ok()?;
    let tier_rates = usage::hit_rate_by_tier(conn).unwrap_or_default();
    let type_rates = usage::hit_rate_by_type(conn).unwrap_or_default();

    Some(UsageReport {
        pack_hits: pack.hits,
        pack_misses: pack.misses,
        search_hits: search.hits,
        search_misses: search.misses,
        pending: pack.pending + search.pending,
        dead_weight,
        never_retrieved_files,
        tier_rates,
        type_rates,
    })
}

fn check_integrity(conn: &Connection) -> bool {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap_or_else(|_| "error".to_string());
    result == "ok"
}

fn check_models(config: &Config, models_dir: &Path) -> bool {
    let code_ok = crate::embed::is_model_cached(&config.embedding.code_model, models_dir);
    let knowledge_ok = crate::embed::is_model_cached(&config.embedding.knowledge_model, models_dir);
    code_ok && knowledge_ok
}

fn check_file_sync(
    conn: &Connection,
    cogz_dir: &Path,
    repo_root: &Path,
    report: &mut DoctorReport,
) {
    // Find DB entities with file_path that no longer exist on disk.
    // File-backed entities (observation, rule, knowledge) have paths
    // relative to cogz_dir. Code entities (function, class, file,
    // module) have paths relative to repo_root.
    // Exclude stale entities — file-backed entities are marked stale
    // precisely when their file has been deleted, so a missing file
    // for a stale entity is the expected state, not an issue.
    let sql = "SELECT id, type, file_path FROM entities WHERE file_path IS NOT NULL AND status NOT IN ('pruned', 'stale')";
    if let Ok(mut stmt) = conn.prepare(sql)
        && let Ok(rows) = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let etype: String = row.get(1)?;
            let file_path: String = row.get(2)?;
            Ok((id, etype, file_path))
        })
    {
        for row in rows.flatten() {
            let (id, etype, file_path) = row;
            let base = match etype.as_str() {
                "function" | "class" | "file" | "module" => repo_root,
                _ => cogz_dir,
            };
            let full_path = base.join(&file_path);
            if !full_path.exists() {
                report.issues.push(Issue {
                    kind: IssueKind::MissingFile,
                    entity_id: Some(id),
                    message: format!("file missing: {}", file_path),
                });
            }
        }
    }

    // Find files on disk that have no DB entity (not synced).
    // Batch-query all file paths at once instead of one query per file.
    let entity_files = crate::files::scan_entity_files(cogz_dir);
    let rel_paths: Vec<String> = entity_files
        .iter()
        .map(|p| {
            p.strip_prefix(cogz_dir)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let synced_paths: std::collections::HashSet<String> =
        batch_query_file_paths(conn, &rel_paths).unwrap_or_default();

    for rel in &rel_paths {
        if !synced_paths.contains(rel) {
            report.issues.push(Issue {
                kind: IssueKind::MissingEntity,
                entity_id: None,
                message: format!("file not synced: {}", rel),
            });
        }
    }
}

/// Batch-query which file paths exist in the entities table. Returns
/// the set of paths that have a matching entity. Chunks to respect
/// SQLite's variable number limit.
fn batch_query_file_paths(
    conn: &Connection,
    paths: &[String],
) -> Result<std::collections::HashSet<String>, rusqlite::Error> {
    if paths.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    const CHUNK_SIZE: usize = 999;
    let mut result = std::collections::HashSet::new();

    for chunk in paths.chunks(CHUNK_SIZE) {
        let placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql =
            format!("SELECT DISTINCT file_path FROM entities WHERE file_path IN ({placeholders})");
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0))?;
        for row in rows {
            result.insert(row?);
        }
    }

    Ok(result)
}

fn check_observation_edits(conn: &Connection, cogz_dir: &Path, report: &mut DoctorReport) {
    // Observations are append-only. If the file's content hash differs
    // from the DB's content_hash, the observation was edited.
    let sql = "SELECT id, file_path, content_hash FROM entities WHERE type = 'observation' AND file_path IS NOT NULL AND status != 'pruned'";
    if let Ok(mut stmt) = conn.prepare(sql)
        && let Ok(rows) = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let file_path: String = row.get(1)?;
            let db_hash: Option<String> = row.get(2)?;
            Ok((id, file_path, db_hash))
        })
    {
        for row in rows.flatten() {
            let (id, file_path, db_hash) = row;
            let full_path = cogz_dir.join(&file_path);
            if !full_path.exists() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let file_hash = crate::files::content_hash(&content);
                if let Some(db_h) = &db_hash
                    && db_h != &file_hash
                {
                    report.issues.push(Issue {
                        kind: IssueKind::ObservationEdited,
                        entity_id: Some(id),
                        message: format!(
                            "observation content hash mismatch (file was edited): {}",
                            file_path
                        ),
                    });
                }
            }
        }
    }
}

/// `volatility: record` outside the natural record classes exempts the
/// entity from every drift/stale mechanism forever — silent and
/// invisible in every queue. Legitimate overrides exist (e.g. a
/// knowledge entry that is genuinely dated), so this warns without
/// blocking.
fn check_volatility_overrides(conn: &Connection, report: &mut DoctorReport) {
    let sql = "SELECT id, type, json_extract(properties,'$.category') \
               FROM entities \
               WHERE json_extract(properties,'$.volatility') = 'record' \
               AND type IN ('rule','knowledge') \
               AND status NOT IN ('rejected','pruned','superseded')";
    if let Ok(mut stmt) = conn.prepare(sql)
        && let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
    {
        for row in rows.flatten() {
            let (id, etype, category) = row;
            // A knowledge entry in a record category with an explicit
            // record override is redundant but consistent — only flag
            // when the override changes the classification.
            let natural_record = etype == "knowledge"
                && category
                    .is_some_and(|c| crate::volatility::RECORD_CATEGORIES.contains(&c.as_str()));
            if !natural_record {
                report.issues.push(Issue {
                    kind: IssueKind::VolatilityFootgun,
                    entity_id: Some(id),
                    message: format!(
                        "volatility: record on a {etype} exempts it from all drift tracking — confirm this is intended dated history"
                    ),
                });
            }
        }
    }
}

/// Category strings are free-form, so "unknown" can't be a fixed-list
/// check — it flags resemblance instead: a category one edit away from
/// a record category silently classifies `living` instead of `record`,
/// and a near-duplicate of a common category is likely a typo.
fn check_category_typos(conn: &Connection, report: &mut DoctorReport) {
    let sql = "SELECT id, json_extract(properties,'$.category') \
               FROM entities WHERE type = 'knowledge' \
               AND json_extract(properties,'$.category') IS NOT NULL \
               AND status NOT IN ('rejected','pruned','superseded')";
    let mut counts: std::collections::HashMap<String, usize> = Default::default();
    let mut rows: Vec<(String, String)> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(sql)
        && let Ok(mapped) =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    {
        for row in mapped.flatten() {
            *counts.entry(row.1.to_lowercase()).or_default() += 1;
            rows.push(row);
        }
    }
    for (id, category) in rows {
        let lower = category.to_lowercase();
        // Exact membership short-circuits — sibling record categories
        // (decision/decisions) sit one edit apart and must not flag
        // each other.
        if crate::volatility::RECORD_CATEGORIES.contains(&lower.as_str()) {
            continue;
        }
        let mut flagged = false;
        for record_cat in crate::volatility::RECORD_CATEGORIES {
            let d = edit_distance(&lower, record_cat);
            if lower != *record_cat && d <= 2 {
                report.issues.push(Issue {
                    kind: IssueKind::CategoryTypo,
                    entity_id: Some(id.clone()),
                    message: format!(
                        "category '{category}' is {d} edit(s) from record category '{record_cat}' — this entity classifies as `living`, not `record`"
                    ),
                });
                flagged = true;
                break;
            }
        }
        // Singleton category near a much-used one → likely a typo.
        let count = counts.get(&lower).copied().unwrap_or(0);
        let near_common = counts
            .iter()
            .filter(|(c, _)| c.as_str() != lower)
            .filter(|(_, n)| **n >= 3)
            .find(|(c, _)| edit_distance(&lower, c) <= 2);
        if !flagged
            && count <= 2
            && let Some((other, _)) = near_common
        {
            report.issues.push(Issue {
                kind: IssueKind::CategoryTypo,
                entity_id: Some(id),
                message: format!(
                    "category '{category}' ({count} entries) resembles '{other}' — possible typo"
                ),
            });
        }
    }
}

/// Classic Levenshtein over chars — category strings are short enough
/// that the quadratic bound is irrelevant.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(ca != cb))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}

fn check_orphaned_supersedes(conn: &Connection, report: &mut DoctorReport) {
    // A superseded entity should have a `superseded_by` property
    // pointing to the entity that replaced it. Merge stores this as a
    // JSON property (and in frontmatter), not as an edge — all edges
    // are redirected to the survivor. If the property is missing, it's
    // an orphaned supersede.
    let sql = "SELECT id, type, properties FROM entities WHERE status = 'superseded'";
    if let Ok(mut stmt) = conn.prepare(sql)
        && let Ok(rows) = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let etype: String = row.get(1)?;
            let props: String = row.get(2)?;
            Ok((id, etype, props))
        })
    {
        for row in rows.flatten() {
            let (id, etype, props) = row;
            let has_superseded_by = serde_json::from_str::<serde_json::Value>(&props)
                .ok()
                .and_then(|v| v.get("superseded_by").cloned())
                .is_some_and(|v| !v.is_null());
            if !has_superseded_by {
                report.issues.push(Issue {
                    kind: IssueKind::OrphanedSupersede,
                    entity_id: Some(id),
                    message: format!("{} is superseded but has no superseded_by property", etype),
                });
            }
        }
    }
}
