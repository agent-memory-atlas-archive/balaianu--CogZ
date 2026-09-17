//! Helper functions extracted from hybrid.rs for file-size compliance.

use rusqlite::Connection;

use crate::storage::crud::{Entity, EntityType, get_entities_batch};
use crate::storage::embeddings::{EmbeddingSpace, knn_search_with_filters};

use super::SearchError;
use crate::search::ChannelSignals;

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
    entity_map: &mut std::collections::HashMap<String, Entity>,
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
    let mut filtered_ids = Vec::new();
    let mut filtered_distances = Vec::new();
    for (id, dist) in knn_results {
        filtered_ids.push(id);
        filtered_distances.push(dist);
    }
    Ok((filtered_ids, filtered_distances))
}
