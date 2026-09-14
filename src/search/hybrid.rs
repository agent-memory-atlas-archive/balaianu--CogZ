//! Hybrid search — FTS5 + dual-model vector search fused via RRF,
//! with optional graph expansion.
//!
//! Knowledge and code entities live in separate embedding spaces.
//! The search function runs KNN per space (when the corresponding
//! query embedding is available) and fuses all ranked lists via RRF.
//!
//! Source-type balancing: FTS results are split by entity type (code
//! vs knowledge) and fused separately. KNN similarity scores from
//! each embedding space determine the merge proportion — a query that
//! is semantically closer to code entities gets a higher code weight.
//! This prevents text-dense knowledge entries from dominating FTS
//! ranking when the query is about code.

use std::collections::{HashMap, HashSet};

#[path = "hybrid_helpers.rs"]
mod hybrid_helpers;

use rusqlite::Connection;

use crate::config::SearchConfig;
use crate::storage::crud::{Entity, get_entities_batch};
use crate::storage::embeddings::EmbeddingSpace;
use crate::storage::query::fts_search;

use super::QueryEmbeddings;
use super::SearchError;
use super::balance::{channel_strength, mean_top_k_similarity};
use super::describe::build_path_descriptions_batch;
use super::expand::{edge_weight, expand_with_paths};
use super::rank::{
    ChannelInput, apply_post_merge, merge_channels, normalize_scores, resolve_channel_weights,
};
use super::rrf::fuse;
use super::{SearchMode, SearchParams, SearchResult, SearchResults};
use hybrid_helpers::knn_channel;

/// Run a hybrid search.
///
/// Runs FTS5 always. Runs knowledge KNN when `embeddings.knowledge` is
/// provided, code KNN when `embeddings.code` is provided. FTS results
/// are split by entity type (code vs knowledge) and fused separately
/// with the corresponding KNN results. KNN similarity scores determine
/// the merge proportion between code and knowledge. When `params.expand`
/// is true, follows graph edges from the top results to find related
/// entities.
pub fn search(
    conn: &Connection,
    query: &str,
    embeddings: QueryEmbeddings<'_>,
    params: &SearchParams,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    // Reject empty or whitespace-only queries early. An empty MATCH
    // expression causes an FTS5 syntax error — return an empty result
    // set instead of propagating a DB error.
    if query.trim().is_empty() {
        return Ok(SearchResults::default());
    }

    let limit = params.limit as i64;
    let type_filter = params.entity_type.as_deref();
    let status_filter = hybrid_helpers::resolve_status_filter(params.status.as_deref());

    // 1. FTS search — returns full entities, cached to avoid re-fetching
    let fts_entities = fts_search(
        conn,
        query,
        type_filter,
        status_filter,
        !params.include_tests,
        limit,
        config.fts_title_weight,
    )?;

    // Seed entity_map with FTS results so we don't re-fetch them later
    let mut entity_map: std::collections::HashMap<String, Entity> = fts_entities
        .iter()
        .map(|e| (e.id.clone(), e.clone()))
        .collect();

    // 2. Split FTS results by entity type (code vs knowledge)
    let (code_fts_ids, knowledge_fts_ids) = hybrid_helpers::split_fts_by_type(&fts_entities);

    // 3. Knowledge vector search (returns IDs + distances for proportion detection)
    let (knowledge_ids, knowledge_distances) = if let Some(k_emb) = embeddings.knowledge {
        knn_channel(
            conn,
            k_emb,
            EmbeddingSpace::Knowledge,
            &mut entity_map,
            type_filter,
            status_filter,
            params.include_tests,
            limit,
        )?
    } else {
        (Vec::new(), Vec::new())
    };

    // 4. Code vector search
    let (code_ids, code_distances) = if let Some(c_emb) = embeddings.code {
        knn_channel(
            conn,
            c_emb,
            EmbeddingSpace::Code,
            &mut entity_map,
            type_filter,
            status_filter,
            params.include_tests,
            limit,
        )?
    } else {
        (Vec::new(), Vec::new())
    };

    // 5. Determine search mode
    let search_mode = match (embeddings.knowledge.is_some(), embeddings.code.is_some()) {
        (true, true) => SearchMode::Hybrid,
        (true, false) => SearchMode::KnowledgeHybrid,
        (false, true) => SearchMode::CodeHybrid,
        (false, false) => SearchMode::FtsOnly,
    };

    // 5b. Per-channel signals, computed once — used by the silence
    //     gate below and exposed in the response so callers can see
    //     why results were weighted, filtered, or silenced.
    let signals = if search_mode == SearchMode::FtsOnly {
        None
    } else {
        Some(super::ChannelSignals {
            code_strength: channel_strength(&code_distances),
            knowledge_strength: channel_strength(&knowledge_distances),
            code_gradient: mean_top_k_similarity(&code_distances, 5),
            knowledge_gradient: mean_top_k_similarity(&knowledge_distances, 5),
        })
    };

    // 5c. Silence gate: when no embedding space produced a distinctive
    //     match (within-batch gradient flat on every active channel),
    //     the honest answer is an empty result — not a normalized list
    //     of garbage. FTS-only skips the gate: with no vector signal
    //     there is nothing to judge by.
    if let Some(s) = &signals
        && config.silence_threshold > 0.0
        && s.code_gradient < config.silence_threshold
        && s.knowledge_gradient < config.silence_threshold
    {
        return Ok(SearchResults {
            results: Vec::new(),
            search_mode,
            filtered_count: fts_entities.len() + code_ids.len() + knowledge_ids.len(),
            signals: Some(s.clone()),
        });
    }

    // 6. Resolve per-channel merge weights for the configured
    //    strategy (see rank::resolve_channel_weights).
    let (code_prop, knowledge_prop) = resolve_channel_weights(
        conn,
        config,
        search_mode,
        &ChannelInput {
            distances: &code_distances,
            fts_count: code_fts_ids.len(),
            knn_count: code_ids.len(),
        },
        &ChannelInput {
            distances: &knowledge_distances,
            fts_count: knowledge_fts_ids.len(),
            knn_count: knowledge_ids.len(),
        },
    )?;

    tracing::debug!(
        code_proportion = code_prop,
        knowledge_proportion = knowledge_prop,
        search_mode = search_mode.as_str(),
        merge_strategy = config.merge_strategy.as_str(),
        "channel weights resolved"
    );

    // 7. Fuse per source type with original RRF weights (no proportion
    //    scaling). Proportions are applied after normalization, not to
    //    the RRF weights, so that uneven weights (e.g. vec_weight=0.6
    //    vs code_vec_weight=0.3) don't counteract the balance.
    let code_fused = if !code_fts_ids.is_empty() || !code_ids.is_empty() {
        let mut lists: Vec<(&[String], f64)> = Vec::new();
        if !code_fts_ids.is_empty() {
            lists.push((&code_fts_ids, config.fts_weight));
        }
        if !code_ids.is_empty() {
            lists.push((&code_ids, config.code_vec_weight));
        }
        fuse(&lists, config.rrf_k)
    } else {
        Vec::new()
    };

    let knowledge_fused = if !knowledge_fts_ids.is_empty() || !knowledge_ids.is_empty() {
        let mut lists: Vec<(&[String], f64)> = Vec::new();
        if !knowledge_fts_ids.is_empty() {
            lists.push((&knowledge_fts_ids, config.fts_weight));
        }
        if !knowledge_ids.is_empty() {
            lists.push((&knowledge_ids, config.vec_weight));
        }
        fuse(&lists, config.rrf_k)
    } else {
        Vec::new()
    };

    // 8. Normalize each fused list to [0, 1], apply channel weights,
    //    and drop entries below the relevance floor. Under the
    //    "strength" strategy the weights are absolute match strengths,
    //    not quota shares — a channel whose best match is weak
    //    contributes low-scored results that the floor then removes.
    let code_normalized = normalize_scores(&code_fused);
    let knowledge_normalized = normalize_scores(&knowledge_fused);

    let (fused, mut filtered_count) = merge_channels(
        code_normalized,
        knowledge_normalized,
        code_prop,
        knowledge_prop,
        config.min_relevance,
    );

    // 8b–8d. Post-merge ranking: provenance prior, MMR
    //    diversification, and the diversity-slot guarantee.
    let fused = apply_post_merge(
        conn,
        fused,
        config,
        search_mode,
        code_prop,
        knowledge_prop,
        params.limit as usize,
    )?;

    // 9. Select top results
    let top_n: Vec<(String, f64)> = fused
        .into_iter()
        .take(params.limit as usize)
        .map(|(id, s, _)| (id, s))
        .collect();

    // 10. Build direct search results (entities already in entity_map)
    let mut results: Vec<SearchResult> = top_n
        .iter()
        .filter_map(|(id, score)| {
            entity_map.get(id).map(|entity| SearchResult {
                entity: entity.clone(),
                relevance: *score as f32,
                graph_path: vec![id.clone()],
                graph_path_description: String::new(),
            })
        })
        .collect();

    // Track access counts for direct results only (derived state for
    // composite scoring). Graph expansions are context, not retrieval.
    let accessed_ids: Vec<String> = results.iter().map(|r| r.entity.id.clone()).collect();
    if !accessed_ids.is_empty() {
        let _ = crate::storage::access::increment_access_batch(conn, &accessed_ids);
    }

    // Extend with expanded results after access tracking.
    if params.expand && params.max_hops > 0 && !results.is_empty() {
        let seed_ids: Vec<String> = results.iter().map(|r| r.entity.id.clone()).collect();
        let exclude_ids: HashSet<String> = results.iter().map(|r| r.entity.id.clone()).collect();

        // Build seed relevance map for decayed scoring of expanded entities
        let seed_relevance: HashMap<String, f32> = results
            .iter()
            .map(|r| (r.entity.id.clone(), r.relevance))
            .collect();

        let expansions = expand_with_paths(
            conn,
            &seed_ids,
            params.max_hops,
            &exclude_ids,
            status_filter,
            params.include_tests,
        )?;

        let uncached_expansion_ids: Vec<String> = expansions
            .iter()
            .map(|e| e.entity_id.clone())
            .filter(|id| !entity_map.contains_key(id))
            .collect();
        let fetched = get_entities_batch(conn, &uncached_expansion_ids)?;
        for entity in fetched {
            entity_map.insert(entity.id.clone(), entity);
        }

        let paths_to_describe: Vec<Vec<String>> =
            expansions.iter().map(|e| e.graph_path.clone()).collect();
        let descriptions = build_path_descriptions_batch(conn, &paths_to_describe)?;

        let mut expanded_results: Vec<SearchResult> = Vec::new();
        let mut seen_expanded: HashSet<String> = HashSet::new();
        for (exp, desc) in expansions.into_iter().zip(descriptions) {
            if !seen_expanded.insert(exp.entity_id.clone()) {
                continue;
            }
            if let Some(entity) = entity_map.get(&exp.entity_id) {
                // Relevance decays aggressively with hop distance: 0.3^hops,
                // multiplied by edge-type weights so curated semantic edges
                // (references, supports, ...) outrank mass structural fan-out
                // (imports, contains) in the expansion cap.
                // 1-hop: 30% × edge weight, 2-hop: 9% × edge weights.
                let hop = exp.graph_path.len().saturating_sub(1);
                let seed_score = seed_relevance.get(&exp.seed_id).copied().unwrap_or(0.0);
                let edge_w: f32 = if config.edge_weighted_expansion {
                    exp.edge_path.iter().map(|t| edge_weight(t)).product()
                } else {
                    1.0
                };
                let decayed = seed_score * 0.3_f32.powi(hop as i32) * edge_w;
                if decayed < config.min_relevance as f32 {
                    filtered_count += 1;
                    continue;
                }
                expanded_results.push(SearchResult {
                    entity: entity.clone(),
                    relevance: decayed,
                    graph_path: exp.graph_path,
                    graph_path_description: desc,
                });
            }
        }

        // Cap expanded results to prevent graph fan-out from flooding
        // the result set. Expanded entities are context, not primary
        // matches — a small number suffices.
        let max_expansions = params.limit as usize;
        if expanded_results.len() > max_expansions {
            expanded_results.sort_by(|a, b| {
                b.relevance
                    .partial_cmp(&a.relevance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            expanded_results.truncate(max_expansions);
        }

        results.extend(expanded_results);
    }

    Ok(SearchResults {
        results,
        search_mode,
        filtered_count,
        signals,
    })
}

#[cfg(test)]
#[path = "hybrid_tests.rs"]
mod tests;
