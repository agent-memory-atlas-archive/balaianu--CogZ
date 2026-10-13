//! Rename/move re-anchoring for declared code references.
//!
//! `code_entity_uuid` keys on `{file_path}:{type}:{qualified_name}`, so
//! a content-preserving move produces a new UUID — the old reference
//! reads `missing` and the living entity gets flagged `code_orphaned`
//! even though the code still exists, relocated. The `v2:` structural
//! hash makes relocation detectable: an active entity carrying the
//! same hash is the same code.
//!
//! Matching is hash-exact only: renames change identifier leaf text
//! and therefore the hash — a renamed anchor genuinely changed what it
//! points at, so it stays `missing` for the review path.
//!
//! Records are never re-anchored — a record's references describe the
//! code as it was when the record was written.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::files::{FmValue, read_entity_file, write_entity_file};
use crate::index::refstate::resolve_file_path;
use crate::index::sync::{is_structural_hash, legacy_content_hash};
use crate::storage::Storage;
use crate::storage::events::{EventType, record_event};
use crate::volatility;

/// Outcome of a re-anchor pass.
#[derive(Debug, Default)]
pub struct ReanchorStats {
    /// (referrer, ref) pairs rewired to a live entity.
    pub reanchored: usize,
    /// Distinct knowledge files rewritten.
    pub files_rewritten: usize,
    /// Referenced stale ids skipped — zero or multiple hash matches.
    pub unresolved: usize,
}

struct StaleCodeRow {
    id: String,
    entity_type: String,
    content_hash: String,
    qualified_name: String,
}

struct ActiveCandidate {
    id: String,
    content: String,
    content_hash: String,
    entity_type: String,
    qualified_name: String,
    language: String,
}

fn qualified_name_of(properties: &serde_json::Value, title: &str) -> String {
    properties
        .get("qualified_name")
        .and_then(|v| v.as_str())
        .or_else(|| properties.get("module_path").and_then(|v| v.as_str()))
        .unwrap_or(title)
        .to_string()
}

/// Rewire declared references that point at stale code entities to an
/// active entity carrying the same semantic hash. Mutates `declared`
/// in place so downstream drift recompute sees the new anchors.
pub fn reanchor_moved_entities(
    storage: &Storage,
    cogz_dir: &Path,
    declared: &mut HashMap<String, Vec<String>>,
) -> ReanchorStats {
    let mut stats = ReanchorStats::default();
    if declared.is_empty() {
        return stats;
    }

    // Referenced ids that are stale code entities — the orphan-risk set.
    let referenced: HashSet<String> = declared.values().flatten().cloned().collect();
    let (stale_rows, referrer_meta) = {
        let conn = storage.conn();
        let stale_rows = load_stale_code_rows(&conn, &referenced);
        if stale_rows.is_empty() {
            return stats;
        }
        let referrer_ids: Vec<String> = declared.keys().cloned().collect();
        (stale_rows, load_referrer_meta(&conn, &referrer_ids))
    };

    // Candidates: active code entities. v2 stale hashes match columns
    // directly; legacy hashes compare via name-preselection plus a
    // recomputed legacy hash of the candidate's stored content.
    let candidates = {
        let conn = storage.conn();
        load_active_candidates(&conn, &stale_rows)
    };
    let mut by_hash: HashMap<&str, Vec<&ActiveCandidate>> = HashMap::new();
    let mut by_name: HashMap<(&str, &str), Vec<&ActiveCandidate>> = HashMap::new();
    for c in &candidates {
        by_hash.entry(c.content_hash.as_str()).or_default().push(c);
        by_name
            .entry((c.entity_type.as_str(), c.qualified_name.as_str()))
            .or_default()
            .push(c);
    }

    // Resolve each stale row to at most one live target.
    let mut reanchors: HashMap<String, (String, String)> = HashMap::new();
    for stale in &stale_rows {
        let pool: Vec<&ActiveCandidate> = if is_structural_hash(&stale.content_hash) {
            by_hash
                .get(stale.content_hash.as_str())
                .cloned()
                .unwrap_or_default()
        } else {
            by_name
                .get(&(stale.entity_type.as_str(), stale.qualified_name.as_str()))
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|c| legacy_content_hash(&c.content, &c.language) == stale.content_hash)
                .collect()
        };
        // Disambiguate identical-content duplicates by qualified name.
        let chosen: Option<&ActiveCandidate> = match pool.len() {
            0 => None,
            1 => Some(pool[0]),
            _ => {
                let named: Vec<&&ActiveCandidate> = pool
                    .iter()
                    .filter(|c| {
                        c.entity_type == stale.entity_type
                            && c.qualified_name == stale.qualified_name
                    })
                    .collect();
                if named.len() == 1 {
                    Some(*named[0])
                } else {
                    None
                }
            }
        };
        match chosen {
            Some(c) => {
                reanchors.insert(stale.id.clone(), (c.id.clone(), c.content_hash.clone()));
            }
            None => {
                stats.unresolved += 1;
            }
        }
    }
    if reanchors.is_empty() {
        return stats;
    }

    let _file_lock = storage.file_lock();

    // Rewrite every living referrer that declares the stale id.
    for (entity_id, refs) in declared.iter_mut() {
        let Some(meta) = referrer_meta.get(entity_id) else {
            continue;
        };
        let hits: Vec<String> = refs
            .iter()
            .filter(|r| reanchors.contains_key(*r))
            .cloned()
            .collect();
        if hits.is_empty() || meta.record {
            continue;
        }
        let path = resolve_file_path(&meta.file_path, cogz_dir);
        let Ok(mut ef) = read_entity_file(&path) else {
            continue;
        };
        // Canonical file first: `declared` is only mutated after the
        // write + resync succeed, so a failure can't split state.
        let mut new_refs = ef.references.clone();
        let mut touched = false;
        for r in new_refs.iter_mut() {
            if let Some((new_id, _)) = reanchors.get(r) {
                *r = new_id.clone();
                touched = true;
            }
        }
        if !touched {
            continue;
        }
        new_refs.sort();
        new_refs.dedup();
        ef.references = new_refs;
        // Re-stamp the provenance entry onto the new anchor + hash.
        for (_, value) in ef
            .frontmatter
            .entries
            .iter_mut()
            .filter(|(k, _)| k == "verified_against")
        {
            if let FmValue::Array(entries) = value {
                for entry in entries.iter_mut() {
                    if let Some((k, _)) = entry.split_once('=')
                        && let Some((new_id, new_hash)) = reanchors.get(k)
                    {
                        *entry = format!("{new_id}={new_hash}");
                    }
                }
                entries.sort();
                entries.dedup();
            }
        }
        ef.updated_at = chrono::Utc::now().to_rfc3339();
        if write_entity_file(&path, &ef).is_err() {
            continue;
        }
        let rel = path.strip_prefix(cogz_dir).unwrap_or(&path);
        let sync = crate::files::sync_single_file(storage, cogz_dir, &rel.to_string_lossy());
        if !sync.errors.is_empty() {
            continue;
        }
        *refs = ef.references.clone();
        stats.files_rewritten += 1;
        stats.reanchored += hits.len();
        for old in hits {
            if let Some((new_id, _)) = reanchors.get(&old) {
                let conn = storage.conn();
                let _ = record_event(
                    &conn,
                    EventType::ReferenceReanchored,
                    Some(entity_id),
                    &serde_json::json!({
                        "entity_id": entity_id,
                        "old_ref": old,
                        "new_ref": new_id,
                    }),
                );
            }
        }
    }
    stats
}

struct ReferrerMeta {
    file_path: String,
    record: bool,
}

fn load_stale_code_rows(
    conn: &rusqlite::Connection,
    referenced: &HashSet<String>,
) -> Vec<StaleCodeRow> {
    if referenced.is_empty() {
        return Vec::new();
    }
    let ids: Vec<&String> = referenced.iter().collect();
    let mut out = Vec::new();
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, type, content_hash, properties, \
             COALESCE(title,'') FROM entities \
             WHERE id IN ({placeholders}) AND status = 'stale' \
             AND type IN ('function','class','file','module') \
             AND content_hash IS NOT NULL"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| *id as &dyn rusqlite::ToSql).collect();
        let rows = conn
            .prepare(&sql)
            .and_then(|mut s| {
                s.query_map(params.as_slice(), |r| {
                    let props_raw: String = r.get(3)?;
                    let props: serde_json::Value =
                        serde_json::from_str(&props_raw).unwrap_or(serde_json::json!({}));
                    let title: String = r.get(4)?;
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        qualified_name_of(&props, &title),
                    ))
                })
                .map(|rows| rows.flatten().collect::<Vec<_>>())
            })
            .unwrap_or_default();
        for (id, entity_type, content_hash, qualified_name) in rows {
            out.push(StaleCodeRow {
                id,
                entity_type,
                content_hash,
                qualified_name,
            });
        }
    }
    out
}

fn load_referrer_meta(
    conn: &rusqlite::Connection,
    ids: &[String],
) -> HashMap<String, ReferrerMeta> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, file_path, type, properties FROM entities \
             WHERE id IN ({placeholders}) AND file_path IS NOT NULL"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = conn
            .prepare(&sql)
            .and_then(|mut s| {
                s.query_map(params.as_slice(), |r| {
                    let props_raw: String = r.get(3)?;
                    let props: serde_json::Value =
                        serde_json::from_str(&props_raw).unwrap_or(serde_json::json!({}));
                    let ty: String = r.get(2)?;
                    let record = volatility::classify(&ty, &props) == volatility::RECORD;
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, record))
                })
                .map(|rows| rows.flatten().collect::<Vec<_>>())
            })
            .unwrap_or_default();
        for (id, file_path, record) in rows {
            out.insert(id, ReferrerMeta { file_path, record });
        }
    }
    out
}

fn load_active_candidates(
    conn: &rusqlite::Connection,
    stale_rows: &[StaleCodeRow],
) -> Vec<ActiveCandidate> {
    let mut v2_hashes: Vec<&str> = Vec::new();
    let mut names: HashSet<(&str, &str)> = HashSet::new();
    for s in stale_rows {
        if is_structural_hash(&s.content_hash) {
            v2_hashes.push(s.content_hash.as_str());
        } else if !s.qualified_name.is_empty() {
            names.insert((s.entity_type.as_str(), s.qualified_name.as_str()));
        }
    }
    let mut out = Vec::new();
    // Hash tier: exact column match against active entities.
    for chunk in v2_hashes.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, content, content_hash, type, properties, \
             COALESCE(title,'') FROM entities \
             WHERE status = 'active' AND content_hash IN ({placeholders})"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|h| h as &dyn rusqlite::ToSql).collect();
        out.extend(query_candidates(conn, &sql, params));
    }
    // Name tier for legacy-hashed stale rows: same type + qualified
    // name, then verify by recomputing the legacy hash on content.
    let name_list: Vec<String> = names.iter().map(|(t, n)| format!("{t}\u{1f}{n}")).collect();
    for chunk in name_list.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, content, content_hash, type, properties, \
             COALESCE(title,'') FROM entities \
             WHERE status = 'active' \
             AND type || char(31) || COALESCE(\
                 json_extract(properties,'$.qualified_name'), \
                 json_extract(properties,'$.module_path'), title, '') \
             IN ({placeholders})"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|n| n as &dyn rusqlite::ToSql).collect();
        out.extend(query_candidates(conn, &sql, params));
    }
    out
}

fn query_candidates(
    conn: &rusqlite::Connection,
    sql: &str,
    params: Vec<&dyn rusqlite::ToSql>,
) -> Vec<ActiveCandidate> {
    conn.prepare(sql)
        .and_then(|mut s| {
            s.query_map(params.as_slice(), |r| {
                let props_raw: String = r.get(4)?;
                let props: serde_json::Value =
                    serde_json::from_str(&props_raw).unwrap_or(serde_json::json!({}));
                let title: String = r.get(5)?;
                Ok(ActiveCandidate {
                    id: r.get(0)?,
                    content: r.get(1)?,
                    content_hash: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    entity_type: r.get(3)?,
                    qualified_name: qualified_name_of(&props, &title),
                    language: props
                        .get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .map(|rows| rows.flatten().collect::<Vec<_>>())
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "reanchor_tests.rs"]
mod tests;
