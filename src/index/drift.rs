//! Knowledge drift — compares each knowledge entity's `verified_against`
//! provenance to the current content hash of every code entity it
//! references. Drift is *derived state* (the `entity_drift` table):
//! recomputed on every index, never written to canonical files.
//!
//! The model, borrowed from Swimm's verified-stamp and Graphiti's
//! invalidate-don't-delete semantics:
//!
//! - `verified_against` (frontmatter, `["<uuid>=<hash>", ...]`) is the
//!   code state the knowledge was last authored or verified against.
//!   It is written at entity creation, on `verify`, and by a one-time
//!   baseline backfill — never by mechanical status transitions.
//! - `entity_drift` holds one row per diverging reference:
//!   `changed` (hash moved), `missing` (target stale/absent),
//!   `unverified` (reference has no recorded baseline).
//! - `stale` status is reserved for orphaned knowledge — entities
//!   whose references point at dead code. Drifted-but-anchored
//!   knowledge stays `active` and retrievable, demoted + annotated.
//! - Recovery: an exact hash revert drains drift automatically on the
//!   next index; `verify_entity` re-stamps provenance explicitly;
//!   orphaned-stale entities heal when all references resolve active.

use std::collections::{HashMap, HashSet};

use crate::files::frontmatter::FmValue;
use crate::index::refstate::{KNOWLEDGE_TYPES, knowledge_entities, target_states_batch};
use crate::storage::{self, Storage};

/// Frontmatter key marking that a stale transition was caused by an
/// orphaned code reference — the only stale cause `heal_stale_entities`
/// is allowed to reverse. Manually-stale entities carry no marker and
/// are never auto-reactivated.
pub const STALE_REASON_ORPHANED: &str = "code_orphaned";

#[derive(Debug, thiserror::Error)]
pub enum DriftError {
    #[error("storage error: {0}")]
    Storage(#[from] storage::StorageError),
    #[error("entity not found: {0}")]
    NotFound(String),
    #[error("entity has no canonical file: {0}")]
    NoFile(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Summary of a `recompute` pass.
#[derive(Debug, Default)]
pub struct DriftStats {
    pub entities_with_drift: usize,
    pub drift_rows: usize,
}

/// Summary of the full post-index maintenance pass.
#[derive(Debug, Default)]
pub struct PostIndexStats {
    pub stale_flagged: usize,
    pub edges_repaired: usize,
    pub verified_backfilled: usize,
    pub drifted_entities: usize,
    pub healed: usize,
    /// Declared references rewired to a live entity after the target
    /// moved (same structural hash) — flagged orphans prevented.
    pub reanchored: usize,
}

/// Parse `verified_against` entries (`"uuid=hash"`) from an entity's
/// DB `properties` JSON or a frontmatter array.
pub use crate::index::post_index::{post_index_pass, stamp_new_reference_hashes};
pub use crate::index::verify::{
    BatchVerifyRow, VerifyOptions, VerifyOutcome, VerifySelection, batch_verify, verify_entity,
};

pub(crate) fn parse_verified_array(entries: &[String]) -> HashMap<String, String> {
    entries
        .iter()
        .filter_map(|e| e.split_once('='))
        .map(|(id, hash)| (id.to_string(), hash.to_string()))
        .collect()
}

pub(crate) fn parse_verified_properties(properties: &serde_json::Value) -> HashMap<String, String> {
    let entries: Vec<String> = properties
        .get("verified_against")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    parse_verified_array(&entries)
}

pub(crate) fn parse_verified_frontmatter(
    fm: &crate::files::frontmatter::Frontmatter,
) -> HashMap<String, String> {
    let entries = fm
        .get("verified_against")
        .and_then(|v| v.as_array())
        .map(|a| a.to_vec())
        .unwrap_or_default();
    parse_verified_array(&entries)
}

pub(crate) fn serialize_verified(map: &HashMap<String, String>) -> FmValue {
    let mut entries: Vec<String> = map.iter().map(|(k, v)| format!("{k}={v}")).collect();
    entries.sort();
    FmValue::Array(entries)
}

/// Recompute the `entity_drift` table from `verified_against`
/// provenance and current reference hashes. Anchors are the *declared*
/// references (canonical frontmatter), not materialized edges — a ref
/// to absent code produces no edge but must still produce a `missing`
/// drift row. Full rebuild — the table is small and correctness beats
/// incrementality here.
pub(crate) fn recompute(storage: &Storage, declared: &HashMap<String, Vec<String>>) -> DriftStats {
    let conn = storage.conn();
    if let Err(e) = conn.execute("DELETE FROM entity_drift", []) {
        tracing::warn!("drift: failed to clear table: {}", e);
        return DriftStats::default();
    }

    let entities = knowledge_entities(&conn);
    let target_ids: Vec<String> = declared
        .values()
        .flat_map(|v| v.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let states = target_states_batch(&conn, &target_ids);

    // Legacy-baseline compat: `verified_against` entries stamped before
    // the v2 structural hash carry the old normalized-content hash.
    // When the entity's column has already migrated, compare the
    // baseline against a freshly computed legacy hash of the stored
    // content — the same comparison the pre-v2 code would have made.
    let mut legacy_needed: HashSet<String> = HashSet::new();
    for (entity_id, properties, _path, _status) in &entities {
        let verified = parse_verified_properties(properties);
        for ref_id in declared.get(entity_id).into_iter().flatten() {
            if verified
                .get(ref_id)
                .is_some_and(|h| !crate::index::sync::is_structural_hash(h))
                && states
                    .get(ref_id)
                    .and_then(|t| t.current_hash.as_deref())
                    .is_some_and(crate::index::sync::is_structural_hash)
            {
                legacy_needed.insert(ref_id.clone());
            }
        }
    }
    let legacy_hashes = legacy_target_hashes(&conn, &legacy_needed);

    let mut stats = DriftStats::default();
    let mut drifted: HashSet<String> = HashSet::new();

    for (entity_id, properties, _path, _status) in &entities {
        let verified = parse_verified_properties(properties);
        for ref_id in declared.get(entity_id).into_iter().flatten() {
            let target = states.get(ref_id);
            let status = target.map(|t| t.status.as_str()).unwrap_or("missing");
            let verified_hash = verified.get(ref_id);
            // Stale knowledge-type targets are navigational refs (lineage
            // to a stale doc): the entity still exists, so fall through to
            // the hash check. Stale/absent CODE targets always mean the
            // referenced code is gone — `missing` regardless of baseline.
            let navigational = target
                .map(|t| KNOWLEDGE_TYPES.contains(&t.entity_type.as_str()))
                .unwrap_or(false);
            let current_hash = match verified_hash {
                Some(h) if !crate::index::sync::is_structural_hash(h) => legacy_hashes
                    .get(ref_id)
                    .or(target.and_then(|t| t.current_hash.as_ref())),
                _ => target.and_then(|t| t.current_hash.as_ref()),
            };
            let cause = if status != "active" && !navigational {
                "missing"
            } else if verified_hash.is_none() {
                "unverified"
            } else if verified_hash != current_hash {
                "changed"
            } else {
                continue;
            };
            if let Err(e) = conn.execute(
                "INSERT OR REPLACE INTO entity_drift \
                 (entity_id, code_id, verified_hash, current_hash, cause) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    entity_id,
                    ref_id,
                    verified_hash,
                    target.and_then(|t| t.current_hash.as_ref()),
                    cause
                ],
            ) {
                tracing::warn!("drift: failed to insert row for {}: {}", entity_id, e);
                continue;
            }
            stats.drift_rows += 1;
            drifted.insert(entity_id.clone());
        }
    }

    stats.entities_with_drift = drifted.len();
    stats
}

/// Recompute the legacy (pre-`v2:`) content hash for entities whose
/// stored hash has already migrated — needed only while comparing
/// against baselines stamped under the old scheme.
fn legacy_target_hashes(
    conn: &rusqlite::Connection,
    ids: &HashSet<String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let ids: Vec<&String> = ids.iter().collect();
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, content, COALESCE(json_extract(properties,'$.language'),'') \
             FROM entities WHERE id IN ({placeholders})"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| *id as &dyn rusqlite::ToSql).collect();
        let rows = conn
            .prepare(&sql)
            .and_then(|mut s| {
                s.query_map(params.as_slice(), |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        r.get::<_, String>(2)?,
                    ))
                })
                .map(|rows| rows.flatten().collect::<Vec<_>>())
            })
            .unwrap_or_default();
        for (id, content, lang) in rows {
            out.insert(id, crate::index::sync::legacy_content_hash(&content, &lang));
        }
    }
    out
}

/// Drift row counts per entity, for retrieval demotion.
pub fn drift_counts(conn: &rusqlite::Connection, entity_ids: &[String]) -> HashMap<String, usize> {
    if entity_ids.is_empty() {
        return HashMap::new();
    }
    let placeholders = entity_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT entity_id, COUNT(*) FROM entity_drift \
         WHERE entity_id IN ({placeholders}) GROUP BY entity_id"
    );
    let params: Vec<&dyn rusqlite::ToSql> = entity_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("drift: failed to prepare counts query: {}", e);
            return HashMap::new();
        }
    };
    match stmt.query_map(params.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
    }) {
        Ok(rows) => rows.flatten().collect(),
        Err(e) => {
            tracing::warn!("drift: failed to query counts: {}", e);
            HashMap::new()
        }
    }
}

/// Entities drifted on references to any of `code_ids` — the inverse
/// of `drift_counts`: instead of "how drifted is this entity", it
/// answers "which entities drifted because *these* code entities
/// moved". The write-time query behind the file_save drift notice:
/// an edit to a file surfaces the knowledge it just invalidated.
/// Returns (entity_id, title) pairs, stable-ordered by title.
pub fn entities_drifted_on(
    conn: &rusqlite::Connection,
    code_ids: &[String],
) -> Vec<(String, String)> {
    if code_ids.is_empty() {
        return Vec::new();
    }
    let placeholders = code_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT DISTINCT e.id, e.title FROM entity_drift d \
         JOIN entities e ON e.id = d.entity_id \
         WHERE d.code_id IN ({placeholders}) ORDER BY e.title"
    );
    let params: Vec<&dyn rusqlite::ToSql> = code_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("drift: failed to prepare drifted-on query: {}", e);
            return Vec::new();
        }
    };
    match stmt.query_map(params.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }) {
        Ok(rows) => rows.flatten().collect(),
        Err(e) => {
            tracing::warn!("drift: failed to query drifted-on: {}", e);
            Vec::new()
        }
    }
}

#[cfg(test)]
#[path = "drift_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "drift_nav_tests.rs"]
mod nav_tests;
