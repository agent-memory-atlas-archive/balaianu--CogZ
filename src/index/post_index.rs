//! Post-index maintenance — the recovery pass that runs after
//! every index/reindex: repair dropped reference edges, flag
//! orphaned knowledge, backfill missing provenance, recompute
//! drift, and heal entities whose anchors came back.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::files::{read_entity_file, write_entity_file};
use crate::index::drift::{PostIndexStats, STALE_REASON_ORPHANED, recompute};
use crate::index::drift::{
    parse_verified_frontmatter, parse_verified_properties, serialize_verified,
};
use crate::index::refstate::{
    KNOWLEDGE_TYPES, declared_references, knowledge_entities, ref_targets_batch, resolve_file_path,
    target_states_batch,
};
use crate::storage::Storage;
use crate::storage::events::{EventType, record_event};

/// Insert `references` edges for declared refs whose targets now exist.
/// Repairs edges dropped by the FK on `edges.target_id` when a file
/// synced before its code targets were indexed — and re-materializes
/// edges after a reverted deletion. Refs to still-absent targets stay
/// unmaterialized; drift tracks them via the declared set instead.
fn repair_reference_edges(storage: &Storage, declared: &HashMap<String, Vec<String>>) -> usize {
    let all_ids: Vec<String> = declared.keys().cloned().collect();
    if all_ids.is_empty() {
        return 0;
    }
    let conn = storage.conn();
    let existing = ref_targets_batch(&conn, &all_ids);
    let targets: HashSet<String> = declared.values().flat_map(|v| v.iter().cloned()).collect();
    let live = target_states_batch(&conn, &targets.iter().cloned().collect::<Vec<_>>());
    let now = chrono::Utc::now().to_rfc3339();
    let mut inserted = 0;
    for (entity_id, refs) in declared {
        let have: HashSet<&str> = existing
            .get(entity_id)
            .into_iter()
            .flatten()
            .map(|t| t.code_id.as_str())
            .collect();
        for ref_id in refs {
            if have.contains(ref_id.as_str()) || !live.contains_key(ref_id) {
                continue;
            }
            let edge = crate::storage::edges::Edge {
                source_id: entity_id.clone(),
                target_id: ref_id.clone(),
                edge_type: "references".to_string(),
                weight: 1.0,
                created_at: now.clone(),
            };
            if crate::storage::edges::insert_edge_skip_fk_violation(&conn, &edge).is_ok() {
                inserted += 1;
            }
        }
    }
    inserted
}

/// Stamp `verified_against` entries for declared references that lack
/// a baseline. Called at entity creation and when `update_knowledge`
/// changes the reference list — the author asserts validity against
/// the referenced entities' current state. Existing entries are never
/// rewritten (only `verify_entity` re-stamps).
///
/// Only references resolving to entities with a `content_hash` are
/// stamped — hashless targets can't drift-detect anyway.
/// Returns the number of entries stamped.
pub fn stamp_new_reference_hashes(
    conn: &rusqlite::Connection,
    entity_file: &mut crate::files::EntityFile,
) -> usize {
    // Record entities are never drift-checked, so provenance baselines
    // on them are dead weight in the canonical file.
    if crate::volatility::classify_file(entity_file) == crate::volatility::RECORD {
        return 0;
    }
    let mut verified = parse_verified_frontmatter(&entity_file.frontmatter);
    let missing: Vec<&String> = entity_file
        .references
        .iter()
        .filter(|r| !verified.contains_key(*r))
        .collect();
    if missing.is_empty() {
        return 0;
    }

    let placeholders = missing.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, COALESCE(json_extract(properties,'$._semantic_hash'), content_hash) \
         FROM entities WHERE id IN ({placeholders}) AND content_hash IS NOT NULL"
    );
    let params: Vec<&dyn rusqlite::ToSql> = missing
        .iter()
        .map(|id| *id as &dyn rusqlite::ToSql)
        .collect();
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return 0;
    };
    let rows = stmt.query_map(params.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    });
    let mut stamped = 0;
    if let Ok(rows) = rows {
        for (id, hash) in rows.flatten() {
            verified.insert(id, hash);
            stamped += 1;
        }
    }
    if stamped > 0 {
        entity_file
            .frontmatter
            .insert("verified_against", serialize_verified(&verified));
    }
    stamped
}

/// Stamp missing `verified_against` entries for knowledge entities.
/// Baseline semantics: entries only ever get *added* here — an entity
/// without provenance is treated as verified against the current code
/// state. Never rewrites an existing entry.
///
/// File-first: writes the canonical file, then re-syncs to the DB.
/// Returns the number of files updated.
pub(crate) fn backfill_verified_against(
    storage: &Storage,
    cogz_dir: &Path,
    declared: &HashMap<String, Vec<String>>,
) -> usize {
    let (candidates, states) = {
        let conn = storage.conn();
        let candidates: Vec<(String, serde_json::Value, String)> = knowledge_entities(&conn)
            .into_iter()
            .filter_map(|(id, props, path, _status)| path.map(|p| (id, props, p)))
            .collect();
        let target_ids: Vec<String> = declared
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        (candidates, target_states_batch(&conn, &target_ids))
    };
    if candidates.is_empty() {
        return 0;
    }

    let mut updated = 0;
    let Ok(_file_lock) = storage.file_lock() else {
        // Maintenance path — a contended lock is retried next pass.
        tracing::warn!("backfill_verified_against: write lock timed out, skipping");
        return 0;
    };

    for (entity_id, properties, file_path) in &candidates {
        // Compute which active refs lack a verified entry.
        let verified = parse_verified_properties(properties);
        let missing: Vec<(String, String)> = declared
            .get(entity_id)
            .into_iter()
            .flatten()
            .filter(|ref_id| !verified.contains_key(*ref_id))
            .filter_map(|ref_id| {
                let t = states.get(ref_id)?;
                (t.status == "active")
                    .then(|| t.current_hash.clone().map(|h| (ref_id.clone(), h)))
                    .flatten()
            })
            .collect();
        if missing.is_empty() {
            continue;
        }

        let path = resolve_file_path(file_path, cogz_dir);
        let Ok(mut entity_file) = read_entity_file(&path) else {
            continue;
        };
        let mut verified = parse_verified_frontmatter(&entity_file.frontmatter);
        for (id, hash) in missing {
            verified.entry(id).or_insert(hash);
        }
        entity_file
            .frontmatter
            .insert("verified_against", serialize_verified(&verified));
        if write_entity_file(&path, &entity_file).is_err() {
            continue;
        }
        let rel = path.strip_prefix(cogz_dir).unwrap_or(&path);
        let _ = crate::files::sync_single_file(storage, cogz_dir, &rel.to_string_lossy());
        updated += 1;
    }
    updated
}

/// Reactivate orphaned-stale knowledge entities whose references all
/// resolve to active code again (deletion reverted, rename re-linked).
/// Manual stale markers are never touched — only entities carrying
/// `stale_reason: code_orphaned`. Record-volatility entities heal
/// unconditionally: the flag was never a legal mark on them, so no
/// reference check is needed to remove it.
pub(crate) fn heal_stale_entities(storage: &Storage, cogz_dir: &Path) -> usize {
    let candidates: Vec<(String, String, bool)> = {
        let conn = storage.conn();
        let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
        let sql = format!(
            "SELECT id, file_path, type, properties FROM entities \
             WHERE type IN ({placeholders}) AND status = 'stale' AND file_path IS NOT NULL"
        );
        let params: Vec<&dyn rusqlite::ToSql> = KNOWLEDGE_TYPES
            .iter()
            .map(|t| t as &dyn rusqlite::ToSql)
            .collect();
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return 0,
        };
        match stmt.query_map(params.as_slice(), |r| {
            let props_raw: String = r.get(3)?;
            let props: serde_json::Value =
                serde_json::from_str(&props_raw).unwrap_or(serde_json::json!({}));
            let ty: String = r.get(2)?;
            let record = crate::volatility::classify(&ty, &props) == crate::volatility::RECORD;
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, record))
        }) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => return 0,
        }
    };
    if candidates.is_empty() {
        return 0;
    }

    let mut healed = 0;
    let Ok(_file_lock) = storage.file_lock() else {
        // Maintenance path — a contended lock is retried next pass.
        tracing::warn!("heal_stale_entities: write lock timed out, skipping");
        return 0;
    };

    for (entity_id, file_path, is_record) in &candidates {
        let path = resolve_file_path(file_path, cogz_dir);
        let Ok(mut entity_file) = read_entity_file(&path) else {
            continue;
        };
        let reason = entity_file
            .frontmatter
            .get("stale_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if reason != STALE_REASON_ORPHANED {
            continue;
        }

        // A `code_orphaned` mark on a record entity is always a
        // misapplied mechanical flag — records were never eligible
        // for it. Heal unconditionally; NLI-owned reasons are
        // untouched either way.
        let record_heal = *is_record;

        // All current anchors must resolve live. Flagging is edge-based
        // — `auto_references` edges flag entities that declare no
        // frontmatter refs at all — so the heal check must union
        // declared refs with auto-link targets, else edge-flagged
        // entities can never recover. An empty union heals the flag:
        // once rebuilt links carry no dead anchor, nothing substantiates
        // `code_orphaned` anymore. Stale knowledge-type targets are
        // navigational (the doc exists), not dead anchors — same
        // semantics as `recompute`.
        let dead = if record_heal {
            0
        } else {
            let conn = storage.conn();
            let mut refs = entity_file.references.clone();
            let mut stmt = match conn.prepare(
                "SELECT target_id FROM edges \
                 WHERE source_id = ?1 AND edge_type = 'auto_references'",
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("heal: auto_ref query failed for {entity_id}: {e}");
                    continue;
                }
            };
            let auto: Vec<String> = stmt
                .query_map([entity_id], |r| r.get(0))
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default();
            refs.extend(auto);
            refs.sort();
            refs.dedup();
            let states = target_states_batch(&conn, &refs);
            refs.iter()
                .filter(|r| match states.get(*r) {
                    Some(t) => {
                        t.status != "active" && !KNOWLEDGE_TYPES.contains(&t.entity_type.as_str())
                    }
                    None => true,
                })
                .count()
        };
        if dead > 0 {
            continue;
        }

        entity_file.status = "active".to_string();
        entity_file
            .frontmatter
            .entries
            .retain(|(k, _)| k != "stale_reason");
        entity_file.updated_at = chrono::Utc::now().to_rfc3339();
        if write_entity_file(&path, &entity_file).is_err() {
            continue;
        }
        let rel = path.strip_prefix(cogz_dir).unwrap_or(&path);
        let sync = crate::files::sync_single_file(storage, cogz_dir, &rel.to_string_lossy());
        if !sync.errors.is_empty() {
            continue;
        }
        let conn = storage.conn();
        let _ = record_event(
            &conn,
            EventType::StaleRecovered,
            Some(entity_id),
            &serde_json::json!({"entity_id": entity_id}),
        );
        healed += 1;
    }
    healed
}

/// provenance, recompute drift, heal recovered orphans. Runs at the
/// end of every index/reindex path (CLI, background, file-save hook).
pub fn post_index_pass(storage: &Storage, cogz_dir: &Path) -> PostIndexStats {
    let mut stats = PostIndexStats::default();

    // Declared references come from canonical files, not edges — edges
    // FK-drop refs to not-yet-indexed targets and can't represent
    // `missing` drift at all.
    let mut declared = declared_references(storage, cogz_dir);

    // Re-anchor first: references to moved code (same structural hash
    // on a live entity) get rewired before flagging, so relocation
    // never reaches `code_orphaned`.
    stats.reanchored =
        crate::index::reanchor::reanchor_moved_entities(storage, cogz_dir, &mut declared)
            .reanchored;

    // Orphan flagging: knowledge referencing code entities that are
    // stale (deleted/removed). Changed-but-live refs produce drift,
    // not stale flags.
    let stale_code_ids: Vec<String> = {
        let conn = storage.conn();
        let mut stmt = match conn.prepare(
            "SELECT id FROM entities \
             WHERE type IN ('function','class','file','module') AND status = 'stale'",
        ) {
            Ok(s) => s,
            Err(_) => return stats,
        };
        match stmt.query_map([], |r| r.get::<_, String>(0)) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => return stats,
        }
    };
    stats.stale_flagged =
        crate::index::stale_flagging::flag_stale_knowledge(storage, cogz_dir, &stale_code_ids);
    stats.edges_repaired = repair_reference_edges(storage, &declared);
    stats.verified_backfilled = backfill_verified_against(storage, cogz_dir, &declared);
    let drift = recompute(storage, &declared);
    stats.drifted_entities = drift.entities_with_drift;
    stats.healed = heal_stale_entities(storage, cogz_dir);
    // A completed sweep — from any caller — counts as a fresh pass for
    // the file-save hook's debounce window.
    let conn = storage.conn();
    let _ = crate::storage::set_meta(
        &conn,
        "last_post_index_pass",
        &chrono::Utc::now().to_rfc3339(),
    );
    stats
}

#[cfg(test)]
#[path = "post_index_tests.rs"]
mod tests;
