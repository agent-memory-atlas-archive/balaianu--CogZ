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
use std::path::{Path, PathBuf};

use crate::files::frontmatter::FmValue;
use crate::files::{read_entity_file, write_entity_file};
use crate::storage::events::{EventType, record_event};
use crate::storage::{self, Storage};

/// Frontmatter key marking that a stale transition was caused by an
/// orphaned code reference — the only stale cause `heal_stale_entities`
/// is allowed to reverse. Manually-stale entities carry no marker and
/// are never auto-reactivated.
pub const STALE_REASON_ORPHANED: &str = "code_orphaned";

const KNOWLEDGE_TYPES: [&str; 3] = ["observation", "rule", "knowledge"];

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
}

/// Parse `verified_against` entries (`"uuid=hash"`) from an entity's
/// DB `properties` JSON or a frontmatter array.
fn parse_verified_array(entries: &[String]) -> HashMap<String, String> {
    entries
        .iter()
        .filter_map(|e| e.split_once('='))
        .map(|(id, hash)| (id.to_string(), hash.to_string()))
        .collect()
}

fn parse_verified_properties(properties: &serde_json::Value) -> HashMap<String, String> {
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

fn parse_verified_frontmatter(
    fm: &crate::files::frontmatter::Frontmatter,
) -> HashMap<String, String> {
    let entries = fm
        .get("verified_against")
        .and_then(|v| v.as_array())
        .map(|a| a.to_vec())
        .unwrap_or_default();
    parse_verified_array(&entries)
}

fn serialize_verified(map: &HashMap<String, String>) -> FmValue {
    let mut entries: Vec<String> = map.iter().map(|(k, v)| format!("{k}={v}")).collect();
    entries.sort();
    FmValue::Array(entries)
}

/// A reference edge target with its current state.
struct RefTarget {
    code_id: String,
    /// Current content hash — `None` when the row is absent.
    current_hash: Option<String>,
    /// Entity status; `"missing"` when no row exists.
    status: String,
    /// Entity type — stale code targets mean missing code; stale
    /// knowledge-type targets are navigational and fall through to
    /// the hash check instead.
    entity_type: String,
}

/// Fetch declared `references` edge targets for a set of source
/// entities in one query, grouped by source. `auto_references` are
/// deliberately excluded — they are derived retrieval hints, not
/// author-asserted anchors, and must never carry a verification burden
/// or pollute `verified_against`.
fn ref_targets_batch(
    conn: &rusqlite::Connection,
    entity_ids: &[String],
) -> HashMap<String, Vec<RefTarget>> {
    let mut out: HashMap<String, Vec<RefTarget>> = HashMap::new();
    if entity_ids.is_empty() {
        return out;
    }
    for chunk in entity_ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT e.source_id, e.target_id, \
             COALESCE(json_extract(t.properties,'$._semantic_hash'), t.content_hash), \
             COALESCE(t.status, 'missing'), COALESCE(t.type, '') \
             FROM edges e LEFT JOIN entities t ON t.id = e.target_id \
             WHERE e.source_id IN ({placeholders}) \
             AND e.edge_type = 'references'"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("drift: failed to prepare batch ref query: {}", e);
                return out;
            }
        };
        let rows = match stmt.query_map(params.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                RefTarget {
                    code_id: r.get(1)?,
                    current_hash: r.get(2)?,
                    status: r.get(3)?,
                    entity_type: r.get(4)?,
                },
            ))
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("drift: failed to query batch refs: {}", e);
                return out;
            }
        };
        for (source_id, target) in rows.flatten() {
            out.entry(source_id).or_default().push(target);
        }
    }
    out
}

/// Load every knowledge entity's *declared* references from its
/// canonical file. Files are the source of truth — `references` edges
/// only cover targets that existed when the file last synced (the FK
/// on `edges.target_id` drops edges to not-yet-indexed code entities,
/// and dropped edges make missing references invisible to drift).
/// Entities whose files can't be read fall back to their materialized
/// edges, which still reflect the last-known declaration.
fn declared_references(storage: &Storage, cogz_dir: &Path) -> HashMap<String, Vec<String>> {
    let files: Vec<(String, String)> = {
        let conn = storage.conn();
        let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
        let sql = format!(
            "SELECT id, file_path FROM entities \
             WHERE type IN ({placeholders}) AND status IN ('active','stale') \
             AND file_path IS NOT NULL"
        );
        let params: Vec<&dyn rusqlite::ToSql> = KNOWLEDGE_TYPES
            .iter()
            .map(|t| t as &dyn rusqlite::ToSql)
            .collect();
        conn.prepare(&sql)
            .and_then(|mut s| {
                s.query_map(params.as_slice(), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    };

    let mut declared: HashMap<String, Vec<String>> = HashMap::new();
    for (id, file_path) in files {
        let path = resolve_file_path(&file_path, cogz_dir);
        match read_entity_file(&path) {
            Ok(ef) => {
                declared.insert(id, ef.references);
            }
            Err(e) => {
                tracing::debug!("drift: declared refs fallback to edges for {}: {}", id, e);
            }
        }
    }

    // Edge fallback for entities whose canonical file is unreadable.
    let missing_ids: Vec<String> = {
        let conn = storage.conn();
        let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
        let sql = format!(
            "SELECT id FROM entities \
             WHERE type IN ({placeholders}) AND status IN ('active','stale')"
        );
        let params: Vec<&dyn rusqlite::ToSql> = KNOWLEDGE_TYPES
            .iter()
            .map(|t| t as &dyn rusqlite::ToSql)
            .collect();
        conn.prepare(&sql)
            .and_then(|mut s| {
                s.query_map(params.as_slice(), |r| r.get::<_, String>(0))
                    .map(|rows| rows.flatten().collect::<Vec<String>>())
            })
            .unwrap_or_default()
    }
    .into_iter()
    .filter(|id| !declared.contains_key(id))
    .collect();

    if !missing_ids.is_empty() {
        let conn = storage.conn();
        let edge_refs = ref_targets_batch(&conn, &missing_ids);
        for (id, targets) in edge_refs {
            declared
                .entry(id)
                .or_insert_with(|| targets.into_iter().map(|t| t.code_id).collect());
        }
    }
    declared
}

/// Fetch `(content_hash, status)` for a set of entity ids in chunks —
/// the target side of a declared reference.
fn target_states_batch(conn: &rusqlite::Connection, ids: &[String]) -> HashMap<String, RefTarget> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, COALESCE(json_extract(properties,'$._semantic_hash'), content_hash), \
             status, type FROM entities WHERE id IN ({placeholders})"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("drift: failed to prepare target query: {}", e);
                return out;
            }
        };
        let rows = match stmt.query_map(params.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                RefTarget {
                    code_id: r.get(0)?,
                    current_hash: r.get(1)?,
                    status: r.get(2)?,
                    entity_type: r.get(3)?,
                },
            ))
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("drift: failed to query target states: {}", e);
                return out;
            }
        };
        out.extend(rows.flatten());
    }
    out
}

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

/// Knowledge entities eligible for drift tracking (excludes terminal
/// statuses). Returns `(id, properties_json, file_path, status)`.
fn knowledge_entities(
    conn: &rusqlite::Connection,
) -> Vec<(String, serde_json::Value, Option<String>, String)> {
    let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
    let sql = format!(
        "SELECT id, properties, file_path, status FROM entities \
         WHERE type IN ({placeholders}) AND status IN ('active','stale')"
    );
    let params: Vec<&dyn rusqlite::ToSql> = KNOWLEDGE_TYPES
        .iter()
        .map(|t| t as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("drift: failed to prepare entity query: {}", e);
            return Vec::new();
        }
    };
    let rows = match stmt.query_map(params.as_slice(), |r| {
        let props_raw: String = r.get(1)?;
        Ok((
            r.get::<_, String>(0)?,
            serde_json::from_str(&props_raw).unwrap_or(serde_json::json!({})),
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("drift: failed to query knowledge entities: {}", e);
            return Vec::new();
        }
    };
    rows.flatten().collect()
}

/// Recompute the `entity_drift` table from `verified_against`
/// provenance and current reference hashes. Anchors are the *declared*
/// references (canonical frontmatter), not materialized edges — a ref
/// to absent code produces no edge but must still produce a `missing`
/// drift row. Full rebuild — the table is small and correctness beats
/// incrementality here.
pub fn recompute(storage: &Storage, declared: &HashMap<String, Vec<String>>) -> DriftStats {
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
            let cause = if status != "active" && !navigational {
                "missing"
            } else if verified_hash.is_none() {
                "unverified"
            } else if verified_hash != target.and_then(|t| t.current_hash.as_ref()) {
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

/// Resolve a DB file_path (relative to `.cogz`) to an absolute path.
fn resolve_file_path(file_path: &str, cogz_dir: &Path) -> PathBuf {
    let normalized = file_path.replace('\\', "/");
    if normalized.starts_with(".cogz/") {
        cogz_dir.parent().unwrap_or(cogz_dir).join(normalized)
    } else {
        cogz_dir.join(normalized)
    }
}

/// Stamp missing `verified_against` entries for knowledge entities.
/// Baseline semantics: entries only ever get *added* here — an entity
/// without provenance is treated as verified against the current code
/// state. Never rewrites an existing entry.
///
/// File-first: writes the canonical file, then re-syncs to the DB.
/// Returns the number of files updated.
pub fn backfill_verified_against(
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
    let _file_lock = storage.file_lock();

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
/// `stale_reason: code_orphaned`.
pub fn heal_stale_entities(storage: &Storage, cogz_dir: &Path) -> usize {
    let candidates: Vec<(String, String)> = {
        let conn = storage.conn();
        let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
        let sql = format!(
            "SELECT id, file_path FROM entities \
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
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => return 0,
        }
    };
    if candidates.is_empty() {
        return 0;
    }

    let mut healed = 0;
    let _file_lock = storage.file_lock();

    for (entity_id, file_path) in &candidates {
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

        // All current anchors must resolve live. Flagging is edge-based
        // — `auto_references` edges flag entities that declare no
        // frontmatter refs at all — so the heal check must union
        // declared refs with auto-link targets, else edge-flagged
        // entities can never recover. An empty union heals the flag:
        // once rebuilt links carry no dead anchor, nothing substantiates
        // `code_orphaned` anymore. Stale knowledge-type targets are
        // navigational (the doc exists), not dead anchors — same
        // semantics as `recompute`.
        let dead = {
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

/// Explicit verification — re-stamps `verified_against` with the
/// current reference hashes, clears the entity's drift rows, and
/// reactivates it when every reference resolves active. This is the
/// recovery primitive: "I checked, still true" as a first-class op.
///
/// Returns `(refs_stamped, reactivated)`.
pub fn verify_entity(
    storage: &Storage,
    cogz_dir: &Path,
    entity_id: &str,
) -> Result<(usize, bool), DriftError> {
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

    let _file_lock = storage.file_lock();
    let mut entity_file =
        read_entity_file(&path).map_err(|_| DriftError::NoFile(file_path.clone()))?;

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
        let dead = refs
            .iter()
            .filter(|r| match states.get(*r) {
                Some(t) => {
                    t.status != "active" && !KNOWLEDGE_TYPES.contains(&t.entity_type.as_str())
                }
                None => true,
            })
            .count();
        (verified, refs.len(), dead)
    };
    let stamped = verified.len();
    entity_file
        .frontmatter
        .insert("verified_against", serialize_verified(&verified));
    let reactivated = prior_status == "stale" && dead == 0;
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
    Ok((stamped, reactivated))
}

/// Post-index maintenance: flag orphaned knowledge, backfill missing
/// provenance, recompute drift, heal recovered orphans. Runs at the
/// end of every index/reindex path (CLI, background, file-save hook).
pub fn post_index_pass(storage: &Storage, cogz_dir: &Path) -> PostIndexStats {
    let mut stats = PostIndexStats::default();

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

    // Declared references come from canonical files, not edges — edges
    // FK-drop refs to not-yet-indexed targets and can't represent
    // `missing` drift at all.
    let declared = declared_references(storage, cogz_dir);
    stats.edges_repaired = repair_reference_edges(storage, &declared);
    stats.verified_backfilled = backfill_verified_against(storage, cogz_dir, &declared);
    let drift = recompute(storage, &declared);
    stats.drifted_entities = drift.entities_with_drift;
    stats.healed = heal_stale_entities(storage, cogz_dir);
    stats
}

#[cfg(test)]
#[path = "drift_tests.rs"]
mod tests;
