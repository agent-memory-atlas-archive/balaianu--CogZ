//! Shared reference/entity state queries for the drift, verify,
//! and post-index modules — batched reads of what the DB knows
//! about declared references, target liveness, and file paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::files::read_entity_file;
use crate::storage::Storage;

/// Entity types that carry file-backed content.
pub(crate) const KNOWLEDGE_TYPES: [&str; 3] = ["observation", "rule", "knowledge"];

pub(crate) struct RefTarget {
    pub(crate) code_id: String,
    /// Current content hash — `None` when the row is absent.
    pub(crate) current_hash: Option<String>,
    /// Entity status; `"missing"` when no row exists.
    pub(crate) status: String,
    /// Entity type — stale code targets mean missing code; stale
    /// knowledge-type targets are navigational and fall through to
    /// the hash check instead.
    pub(crate) entity_type: String,
}

/// Fetch declared `references` edge targets for a set of source
/// entities in one query, grouped by source. `auto_references` are
/// deliberately excluded — they are derived retrieval hints, not
/// author-asserted anchors, and must never carry a verification burden
/// or pollute `verified_against`.
pub(crate) fn ref_targets_batch(
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
pub(crate) fn declared_references(
    storage: &Storage,
    cogz_dir: &Path,
) -> HashMap<String, Vec<String>> {
    let files: Vec<(String, String)> = {
        let conn = storage.conn();
        // Pruned tombstones still declare references in frontmatter;
        // including them lets repair_reference_edges re-materialize
        // their edges after a rebuild. Drift and backfill never act on
        // them — both iterate knowledge_entities (active/stale only).
        let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
        let sql = format!(
            "SELECT id, file_path FROM entities \
             WHERE type IN ({placeholders}) \
             AND status IN ('active','stale','pruned') \
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
             WHERE type IN ({placeholders}) \
             AND status IN ('active','stale','pruned')"
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
pub(crate) fn target_states_batch(
    conn: &rusqlite::Connection,
    ids: &[String],
) -> HashMap<String, RefTarget> {
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

/// Knowledge entities eligible for drift tracking (excludes terminal
/// statuses and `record` volatility — dated facts have no baseline to
/// check). Returns `(id, properties_json, file_path, status)`.
pub(crate) fn knowledge_entities(
    conn: &rusqlite::Connection,
) -> Vec<(String, serde_json::Value, Option<String>, String)> {
    let placeholders = KNOWLEDGE_TYPES.map(|_| "?").join(",");
    let sql = format!(
        "SELECT id, properties, file_path, status, type FROM entities \
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
        let props: serde_json::Value =
            serde_json::from_str(&props_raw).unwrap_or(serde_json::json!({}));
        let ty: String = r.get(4)?;
        let vol = crate::volatility::classify(&ty, &props);
        Ok((
            r.get::<_, String>(0)?,
            props,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            vol,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("drift: failed to query knowledge entities: {}", e);
            return Vec::new();
        }
    };
    rows.flatten()
        .filter(|(_, _, _, _, v)| *v == crate::volatility::LIVING)
        .map(|(id, props, path, status, _)| (id, props, path, status))
        .collect()
}

/// Resolve a DB file_path (relative to `.cogz`) to an absolute path.
pub(crate) fn resolve_file_path(file_path: &str, cogz_dir: &Path) -> PathBuf {
    let normalized = file_path.replace('\\', "/");
    if normalized.starts_with(".cogz/") {
        cogz_dir.parent().unwrap_or(cogz_dir).join(normalized)
    } else {
        cogz_dir.join(normalized)
    }
}
