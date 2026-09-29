//! Helper functions extracted from hybrid.rs for file-size compliance.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::config::SearchConfig;
use crate::storage::crud::{Entity, EntityType, get_entities_batch};
use crate::storage::embeddings::{EmbeddingSpace, knn_search_with_filters};

use super::SearchError;
use crate::search::prf;
use crate::search::{ChannelSignals, SearchParams, SearchResult};

/// Provenance tag on co-change expansion results. The expansion-cap
/// quota partitions on this string — emit site and cap stay in sync
/// through the const.
pub(super) const COCHANGE_DESC: &str = "co-change";

/// Title tokens shared with the query — the overlap signal both
/// expansion channels use to separate a real lead from a file-mate.
fn title_query_overlap(entity: &Entity, query_terms: &HashSet<String>) -> usize {
    entity
        .title
        .as_deref()
        .map(|t| {
            prf::tokenize(t)
                .iter()
                .filter(|x| query_terms.contains(*x))
                .count()
        })
        .unwrap_or(0)
}

/// Silence-gate predicate: true means "no channel produced any
/// evidence of a match" — flat within-batch gradient AND top-3
/// absolute strength below the floor on every channel. The strength
/// clause is the escape hatch for queries whose nearest neighbors are
/// uniformly decent (flat gradient, real matches).
pub(crate) fn should_silence(signals: &ChannelSignals, threshold: f64, floor: f64) -> bool {
    threshold > 0.0
        && signals.code_gradient < threshold
        && signals.knowledge_gradient < threshold
        && signals.code_strength < floor
        && signals.knowledge_strength < floor
}

/// Co-change channel: query terms → entities those terms historically
/// co-changed with. This is the vocabulary bridge commit-style
/// queries need: prose terms ("deprecate app=") map to entities that
/// changed under those terms before, so a query can reach entities
/// sharing zero identifier overlap.
///
/// `before_ts` bounds the history drawn on (None = everything mined).
/// Entities rank by summed term-IDF — a term appearing in few commits
/// is a strong pointer, one in thousands is noise. Returns
/// (entity_id, file_entity_id, idf_score, count) tuples — the file
/// id makes a truthful [file → member] provenance path.
pub(super) fn cochange_candidates(
    conn: &Connection,
    query: &str,
    before_ts: Option<i64>,
    cap: usize,
) -> Result<Vec<(String, String, f64, i64)>, SearchError> {
    let terms = prf::content_terms(query);
    if terms.is_empty() || cap == 0 {
        return Ok(Vec::new());
    }
    let bound = before_ts.unwrap_or(i64::MAX);
    let ph = terms.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    // Entities score by summed inverse document frequency — Σ 1/df
    // where df is the term's commit count in the bound window. A
    // term in a handful of commits ("deprecate") is a strong pointer;
    // one in thousands ("fix") is noise — without this the channel
    // floods with whatever entities generic churn touched.
    let sql = format!(
        "WITH tf AS (
           SELECT term, COUNT(DISTINCT commit_id) AS df
           FROM cochange WHERE term IN ({ph}) AND commit_ts < ?
           GROUP BY term
         )
         SELECT c.entity_id,
                (SELECT f.id FROM entities f
                 WHERE f.file_path = e.file_path AND f.type = 'file'
                 AND f.status = 'active' LIMIT 1) AS file_id,
                SUM(1.0 / tf.df) AS score,
                COUNT(*) AS cnt
         FROM cochange c
         JOIN tf ON tf.term = c.term
         JOIN entities e ON e.id = c.entity_id
         WHERE c.commit_ts < ? AND e.status = 'active'
         GROUP BY c.entity_id
         ORDER BY score DESC, c.entity_id
         LIMIT ?"
    );
    let mut bound_params: Vec<Box<dyn rusqlite::ToSql>> = terms
        .iter()
        .map(|t| Box::new(t.clone()) as Box<dyn rusqlite::ToSql>)
        .collect();
    bound_params.push(Box::new(bound));
    bound_params.push(Box::new(bound));
    bound_params.push(Box::new(cap as i64));
    let params_ref: Vec<&dyn rusqlite::ToSql> = bound_params.iter().map(|b| b.as_ref()).collect();
    let db_err = |e: rusqlite::Error| SearchError::Storage(e.into());
    let out: Vec<(String, String, f64, i64)> = conn
        .prepare(&sql)
        .map_err(db_err)?
        .query_map(params_ref.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(db_err)?
        .filter_map(|row| match row {
            Ok((eid, Some(fid), sc, cnt)) => Some(Ok((eid, fid, sc, cnt))),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_, _>>()
        .map_err(db_err)?;
    Ok(out)
}

/// Split FTS results into code and knowledge entity ID lists.
/// Code entities: function, class, file, module.
/// Knowledge entities: observation, rule, knowledge.
pub(super) fn split_fts_by_type(fts_entities: &[Entity]) -> (Vec<String>, Vec<String>) {
    let mut code = Vec::new();
    let mut knowledge = Vec::new();
    for entity in fts_entities {
        let etype = match EntityType::parse(&entity.r#type) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if etype.is_code() {
            code.push(entity.id.clone());
        } else {
            knowledge.push(entity.id.clone());
        }
    }
    (code, knowledge)
}

/// Resolve the status filter: None and "active" → Some("active"),
/// "all" → None (no filter), anything else → Some(value).
pub(super) fn resolve_status_filter(status: Option<&str>) -> Option<&str> {
    match status {
        None => Some("active"),
        Some("all") => None,
        Some(s) => Some(s),
    }
}

/// Run KNN for one embedding space, push filters into SQL so
/// non-matching entities don't consume KNN slots, and cache fetched
/// entities into `entity_map`.
#[allow(clippy::too_many_arguments)]
pub(super) fn knn_channel(
    conn: &Connection,
    query: &[f32],
    space: EmbeddingSpace,
    entity_map: &mut HashMap<String, Entity>,
    type_filter: Option<&str>,
    status_filter: Option<&str>,
    include_tests: bool,
    limit: i64,
) -> Result<(Vec<String>, Vec<f32>), SearchError> {
    let knn_limit = limit * 3;
    let knn_results = knn_search_with_filters(
        conn,
        space,
        query,
        knn_limit,
        type_filter,
        status_filter,
        !include_tests,
    )?;

    let uncached_ids: Vec<String> = knn_results
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| !entity_map.contains_key(id))
        .collect();
    let fetched = get_entities_batch(conn, &uncached_ids)?;
    for entity in fetched {
        entity_map.insert(entity.id.clone(), entity);
    }

    // Keep the full knn_limit fetch — the wider list gives RRF a real
    // candidate pool; the merge caps output at the caller's limit.
    Ok(knn_results.into_iter().unzip())
}

/// Entities that share a `contains` parent file with an anchor —
/// the file-level siblings of each anchor. Member anchors reach
/// siblings through the shared file; file anchors reach their members
/// directly. Returns `(anchor_id, sibling_id)` pairs; the sibling set
/// includes each anchor itself (filtered downstream by exclude_ids).
pub(super) fn file_siblings(
    conn: &Connection,
    anchor_ids: &[String],
) -> Result<Vec<(String, String)>, SearchError> {
    if anchor_ids.is_empty() {
        return Ok(Vec::new());
    }
    let ph = anchor_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "WITH af AS (
            SELECT target_id AS anchor, source_id AS file
            FROM edges WHERE edge_type = 'contains' AND target_id IN ({ph})
            UNION
            SELECT source_id, source_id
            FROM edges WHERE edge_type = 'contains' AND source_id IN ({ph})
        )
        SELECT DISTINCT af.anchor, m.target_id FROM af
        JOIN edges m ON m.source_id = af.file AND m.edge_type = 'contains'",
    );
    let params: Vec<&dyn rusqlite::ToSql> = anchor_ids
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let params2 = params.clone();
    let db_err = |e: rusqlite::Error| SearchError::Storage(e.into());
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(params.into_iter().chain(params2)),
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(db_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(db_err)?);
    }
    Ok(out)
}

/// File-sibling expansion runs BEFORE the generic expansion loops:
/// enclosing-scope entities (the class of an anchor's file) would
/// otherwise be claimed-and-floored by 2-hop `contains` traversals —
/// marked seen without ever being emitted. Emitting siblings first
/// lets them claim their slots at a score a 2-hop contains path could
/// never reach. Returns the number of candidates dropped by the
/// relevance floor.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_sibling_expansions(
    conn: &Connection,
    results: &[SearchResult],
    config: &SearchConfig,
    params: &SearchParams,
    entity_map: &mut HashMap<String, Entity>,
    exclude_ids: &HashSet<String>,
    seed_relevance: &HashMap<String, f32>,
    query_terms: &HashSet<String>,
    seen_expanded: &mut HashSet<String>,
    expanded_results: &mut Vec<SearchResult>,
) -> Result<usize, SearchError> {
    let mut filtered = 0usize;
    let anchor_ids: Vec<String> = results
        .iter()
        .take(config.sibling_max_anchors)
        .map(|r| r.entity.id.clone())
        .collect();
    let pairs = file_siblings(conn, &anchor_ids)?;
    let uncached: Vec<String> = pairs
        .iter()
        .map(|(_, s)| s.clone())
        .filter(|id| !entity_map.contains_key(id))
        .collect();
    for entity in get_entities_batch(conn, &uncached)? {
        entity_map.insert(entity.id.clone(), entity);
    }
    // Best anchor per sibling keeps best-score-wins on entities
    // reachable from several anchors.
    let mut best: HashMap<String, (String, f32)> = HashMap::new();
    for (anchor, sib) in pairs {
        if exclude_ids.contains(&sib) {
            continue;
        }
        let score = seed_relevance.get(&anchor).copied().unwrap_or(0.0);
        best.entry(sib)
            .and_modify(|(a, s)| {
                if score > *s {
                    *a = anchor.clone();
                    *s = score;
                }
            })
            .or_insert((anchor, score));
    }
    // Only enclosing-scope siblings earn cap space: the class entities
    // of an anchor's file (1-3 per file, the GT's line-range parents),
    // not every function-mate — a bare same-file fan-out floods the
    // expansion cap with ~15 candidates per anchor and displaces
    // pointed hits. Title overlap with the query separates a real
    // lead from a file-mate.
    //
    // HashMap iteration order is randomized per process; sort so
    // emission order (and therefore rank under the cap) is
    // deterministic.
    let mut best_sorted: Vec<(String, (String, f32))> = best.into_iter().collect();
    best_sorted.sort_by(|a, b| {
        b.1.1
            .partial_cmp(&a.1.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    for (sib_id, (anchor_id, seed)) in best_sorted {
        let Some(entity) = entity_map.get(&sib_id) else {
            continue;
        };
        // Type and test checks come before the seen-mark:
        // non-candidate file-mates must not be poisoned out of the
        // other expansion channels.
        if entity.r#type != "class" {
            continue;
        }
        if !params.include_tests
            && entity
                .file_path
                .as_deref()
                .is_some_and(crate::index::is_test_file)
        {
            continue;
        }
        if !seen_expanded.insert(sib_id.clone()) {
            continue;
        }
        let overlap = title_query_overlap(entity, query_terms);
        let decayed = seed * (0.15 + 0.15 * overlap.min(4) as f32);
        if decayed < config.min_relevance as f32 {
            filtered += 1;
            continue;
        }
        expanded_results.push(SearchResult {
            entity: entity.clone(),
            relevance: decayed,
            graph_path: vec![anchor_id, sib_id],
            graph_path_description: "same file".to_string(),
            drift_count: 0,
        });
    }
    Ok(filtered)
}

/// Co-change expansion: member entities of files that query terms
/// historically co-changed with. The only channel that adds genuinely
/// new information (repo history), but its candidates are context, not
/// retrieval — measured: running it as a direct RRF list displaced
/// mid-tail direct hits and cost R@20 on all corpora. Emits before the
/// generic loops so its members aren't claimed-and-floored by graph
/// traversal. Returns the number of candidates dropped by the
/// relevance floor.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_cochange_expansions(
    conn: &Connection,
    query: &str,
    params: &SearchParams,
    config: &SearchConfig,
    entity_map: &mut HashMap<String, Entity>,
    exclude_ids: &HashSet<String>,
    query_terms: &HashSet<String>,
    seen_expanded: &mut HashSet<String>,
    expanded_results: &mut Vec<SearchResult>,
) -> Result<usize, SearchError> {
    let mut filtered = 0usize;
    match cochange_candidates(
        conn,
        query,
        params.before_ts,
        (params.limit as usize).saturating_mul(3),
    ) {
        Ok(members) => {
            let uncached: Vec<String> = members
                .iter()
                .map(|(id, _, _, _)| id.clone())
                .filter(|id| !entity_map.contains_key(id))
                .collect();
            for entity in get_entities_batch(conn, &uncached)? {
                entity_map.insert(entity.id.clone(), entity);
            }
            for (member_id, file_id, idf, count) in members {
                if exclude_ids.contains(&member_id) || seen_expanded.contains(&member_id) {
                    continue;
                }
                let Some(entity) = entity_map.get(&member_id) else {
                    continue;
                };
                if !params.include_tests
                    && entity
                        .file_path
                        .as_deref()
                        .is_some_and(crate::index::is_test_file)
                {
                    continue;
                }
                seen_expanded.insert(member_id.clone());
                let overlap = title_query_overlap(entity, query_terms);
                // IDF total saturates toward 0.15 — keeps strong
                // history leads competitive with graph expansion
                // without letting history flood it.
                let norm = (idf / (idf + 3.0)) as f32;
                let score = 0.07
                    + 0.08 * norm
                    + 0.05 * (overlap.min(3) as f32)
                    + 0.005 * (count.min(8) as f32);
                if score < config.min_relevance as f32 {
                    filtered += 1;
                    continue;
                }
                expanded_results.push(SearchResult {
                    entity: entity.clone(),
                    relevance: score,
                    graph_path: vec![file_id, member_id],
                    graph_path_description: COCHANGE_DESC.to_string(),
                    drift_count: 0,
                });
            }
        }
        Err(e) => {
            tracing::warn!("co-change channel failed, continuing without it: {e}");
        }
    }
    Ok(filtered)
}
