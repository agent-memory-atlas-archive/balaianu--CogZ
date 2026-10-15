//! Explicit verification — operator attestation that a knowledge
//! claim still holds. Verify resolves the entity's declared
//! references mechanically, re-stamps `verified_against` with the
//! current hashes, clears drift rows, and reactivates stale
//! entities whose anchors are all live.

use std::collections::HashMap;
use std::path::Path;

use crate::files::{read_entity_file, write_entity_file};
use crate::index::drift::{DriftError, serialize_verified};
use crate::index::refstate::{KNOWLEDGE_TYPES, resolve_file_path, target_states_batch};
use crate::storage::Storage;
use crate::storage::events::{EventType, record_event};

/// Explicit verification — re-stamps `verified_against` with the
/// current reference hashes, clears the entity's drift rows, and
/// reactivates it when every reference resolves active. This is the
/// recovery primitive: "I checked, still true" as a first-class op.
///
/// Options for `verify_entity`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VerifyOptions {
    /// Reactivate even when some references are dead — the caller
    /// asserts the claim holds despite missing anchors.
    pub allow_dead_refs: bool,
}

/// Result of `verify_entity`.
pub struct VerifyOutcome {
    pub stamped: usize,
    /// References that resolved to a live, hash-stamped target.
    pub resolved: usize,
    /// References that resolve to nothing live (absent or non-active
    /// non-knowledge targets) — the anchors that block reactivation.
    pub dead_refs: Vec<String>,
    pub reactivated: bool,
}

/// Verify a knowledge entity against its references: re-stamps
/// `verified_against` for resolvable refs, clears drift, and
/// reactivates stale entities whose refs are all live. Verification
/// is operator attestation — "I checked, still true" — the anchors
/// resolve mechanically, the claim's truth is asserted by the caller.
pub fn verify_entity(
    storage: &Storage,
    cogz_dir: &Path,
    entity_id: &str,
    opts: &VerifyOptions,
) -> Result<VerifyOutcome, DriftError> {
    let (file_path, prior_status) = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT file_path, status FROM entities WHERE id = ?1",
            [entity_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(|_| DriftError::NotFound(entity_id.to_string()))?
    };
    let file_path = file_path.ok_or_else(|| DriftError::NoFile(entity_id.to_string()))?;
    let path = resolve_file_path(&file_path, cogz_dir);

    let _file_lock = storage.file_lock()?;
    let mut entity_file =
        read_entity_file(&path).map_err(|_| DriftError::NoFile(file_path.clone()))?;
    let is_record = crate::volatility::classify_file(&entity_file) == crate::volatility::RECORD;

    // Re-stamp from the live declared-reference set.
    let (verified, total, dead) = {
        let conn = storage.conn();
        let refs = entity_file.references.clone();
        let states = target_states_batch(&conn, &refs);
        let verified: HashMap<String, String> = refs
            .iter()
            .filter_map(|r| {
                // Stamp any target that exists with a hash — including
                // stale ones, so lineage refs to stale entities can be
                // acknowledged instead of drifting `missing` forever.
                states.get(r)?.current_hash.clone().map(|h| (r.clone(), h))
            })
            .collect();
        // Dead = non-active non-knowledge targets or absent rows. A
        // stale knowledge-type ref is navigational, not a dead anchor —
        // it must not block reactivation (mirrors `recompute`).
        let dead: Vec<String> = refs
            .iter()
            .filter(|r| match states.get(*r) {
                Some(t) => {
                    t.status != "active" && !KNOWLEDGE_TYPES.contains(&t.entity_type.as_str())
                }
                None => true,
            })
            .cloned()
            .collect();
        (verified, refs.len(), dead)
    };

    let stamped = if is_record {
        // Records track no provenance — verify still clears drift and
        // reactivates, but stamps are dead weight and legacy ones get
        // stripped here.
        entity_file
            .frontmatter
            .entries
            .retain(|(k, _)| k != "verified_against");
        0
    } else {
        entity_file
            .frontmatter
            .insert("verified_against", serialize_verified(&verified));
        verified.len()
    };
    // Records reactivate unconditionally on verify — dead anchors
    // can't substantiate staleness of a dated fact. Living entities
    // still require every reference to resolve active, unless the
    // caller explicitly asserts the claim holds anyway.
    let reactivated =
        prior_status == "stale" && (is_record || dead.is_empty() || opts.allow_dead_refs);
    if reactivated {
        entity_file.status = "active".to_string();
        entity_file
            .frontmatter
            .entries
            .retain(|(k, _)| k != "stale_reason");
    }
    entity_file.updated_at = chrono::Utc::now().to_rfc3339();
    write_entity_file(&path, &entity_file).map_err(|_| DriftError::NoFile(file_path.clone()))?;

    let rel = path.strip_prefix(cogz_dir).unwrap_or(&path);
    let _ = crate::files::sync_single_file(storage, cogz_dir, &rel.to_string_lossy());

    let conn = storage.conn();
    let _ = conn.execute("DELETE FROM entity_drift WHERE entity_id = ?1", [entity_id]);
    let _ = record_event(
        &conn,
        EventType::KnowledgeVerified,
        Some(entity_id),
        &serde_json::json!({"entity_id": entity_id, "refs_stamped": stamped, "total_refs": total, "reactivated": reactivated}),
    );
    Ok(VerifyOutcome {
        stamped,
        resolved: verified.len(),
        dead_refs: dead,
        reactivated,
    })
}

/// Which entities a batch verify pass selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifySelection {
    /// Every entity with at least one drift row.
    Drifted,
    /// Every file-backed entity marked stale.
    Stale,
    /// Living entities with no provenance baseline or `unverified`
    /// drift — the coverage gap code-change triggers can't reach.
    Unverified,
    /// Every living file-backed entity.
    All,
}

impl VerifySelection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Drifted => "drifted",
            Self::Stale => "stale",
            Self::Unverified => "unverified",
            Self::All => "all",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "drifted" => Some(Self::Drifted),
            "stale" => Some(Self::Stale),
            "unverified" => Some(Self::Unverified),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// One row of a batch verify result.
pub struct BatchVerifyRow {
    pub entity_id: String,
    pub title: String,
    pub outcome: Result<VerifyOutcome, DriftError>,
}

/// SQL fragment matching living entities: records (observations and
/// record-category/record-override knowledge) are excluded — drift
/// coverage is a living-entity concern. The category list derives
/// from `RECORD_CATEGORIES` so the two cannot drift apart.
fn living_only() -> String {
    let cats = crate::volatility::RECORD_CATEGORIES
        .iter()
        .map(|c| format!("'{c}'"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "type != 'observation' \
         AND COALESCE(json_extract(properties,'$.volatility'),'') != 'record' \
         AND COALESCE(json_extract(properties,'$.category'),'') NOT IN ({cats})"
    )
}

/// Select entity (id, title) pairs for a batch verify pass — one
/// query, no per-entity lookups downstream.
fn select_verify_ids(
    conn: &rusqlite::Connection,
    selection: VerifySelection,
) -> Vec<(String, String)> {
    let file_backed = "file_path IS NOT NULL AND type IN ('observation','rule','knowledge')";
    let sql = match selection {
        VerifySelection::Drifted => {
            "SELECT DISTINCT d.entity_id AS id, COALESCE(e.title,'') AS title \
             FROM entity_drift d LEFT JOIN entities e ON e.id = d.entity_id \
             ORDER BY id"
                .to_string()
        }
        VerifySelection::Stale => {
            format!(
                "SELECT id, COALESCE(title,'') AS title FROM entities \
                 WHERE status='stale' AND {file_backed} ORDER BY id"
            )
        }
        VerifySelection::Unverified => {
            format!(
                "SELECT id, COALESCE(title,'') AS title FROM entities \
                 WHERE {file_backed} AND {} \
                 AND status IN ('active','stale') AND ( \
                 json_extract(properties,'$.verified_against') IS NULL \
                 OR json_extract(properties,'$.verified_against') = '[]' \
                 OR id IN (SELECT entity_id FROM entity_drift WHERE cause='unverified') \
                 ) ORDER BY id",
                living_only()
            )
        }
        VerifySelection::All => {
            format!(
                "SELECT id, COALESCE(title,'') AS title FROM entities \
                 WHERE {file_backed} AND {} \
                 AND status IN ('active','stale') ORDER BY id",
                living_only()
            )
        }
    };
    conn.prepare(&sql)
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map(|m| m.flatten().collect())
        })
        .unwrap_or_else(|e| {
            tracing::warn!("batch verify selection query failed: {e}");
            Vec::new()
        })
}

/// Verify every entity in `selection` sequentially — each call is a
/// full file-first verify.
pub fn batch_verify(
    storage: &Storage,
    cogz_dir: &Path,
    selection: VerifySelection,
    opts: &VerifyOptions,
) -> Vec<BatchVerifyRow> {
    let ids = {
        let conn = storage.conn();
        select_verify_ids(&conn, selection)
    };
    let mut rows = Vec::with_capacity(ids.len());
    for (entity_id, title) in ids {
        let outcome = verify_entity(storage, cogz_dir, &entity_id, opts);
        rows.push(BatchVerifyRow {
            entity_id,
            title,
            outcome,
        });
    }
    rows
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "verify_lifecycle_tests.rs"]
mod lifecycle_tests;
