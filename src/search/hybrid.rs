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
use crate::storage::crud::{Entity, EntityType, get_entities_batch};
use crate::storage::embeddings::EmbeddingSpace;
use crate::storage::query::fts_search;

use super::QueryEmbeddings;
use super::SearchError;
use super::balance::{channel_strength, mean_top_k_similarity};
use super::describe::build_path_descriptions_batch;
use super::expand::{edge_weight, expand_with_paths};
use super::graph_retrieval::graph_retrieve;
use super::prf;
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

    // 1. FTS search — fetch 3× the output limit so the merge sees a
    //    wide candidate pool (RRF arbitrates depth; the result still
    //    caps at `limit`). Missed entities routinely sit at FTS rank
    //    25-80 — inside the index but invisible at a 20-deep fetch.
    let fts_entities = fts_search(
        conn,
        query,
        type_filter,
        status_filter,
        !params.include_tests,
        limit * 3,
        config.fts_title_weight,
    )?;

    // Seed entity_map with FTS results so we don't re-fetch them later
    let mut entity_map: std::collections::HashMap<String, Entity> = fts_entities
        .iter()
        .map(|e| (e.id.clone(), e.clone()))
        .collect();

    // 2. Split FTS results by entity type (code vs knowledge). Only
    //    the top `limit` hits feed the direct channels — deeper hits
    //    join the expansion set later so they add recall without
    //    displacing direct results.
    let direct_window = (params.limit as usize).min(fts_entities.len());
    let (code_fts_ids, knowledge_fts_ids) =
        hybrid_helpers::split_fts_by_type(&fts_entities[..direct_window]);

    // Deep-pool candidates: entities ranked beyond `limit` in any
    // channel. They never enter the direct merge — they are appended
    // to the expansion set as recall-only candidates.
    let mut deep_ids: Vec<String> = fts_entities[direct_window..]
        .iter()
        .map(|e| e.id.clone())
        .collect();

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

    // 4b. Pseudo-relevance feedback: expand the query with terms
    //     mined from the top FTS hits and re-run FTS. Novel hits are
    //     appended later as expansion-style results — they count for
    //     recall but can never displace direct results. Targets
    //     vocabulary-mismatch misses the original phrasing can't
    //     reach. Works in FTS-only mode; no-op when FTS has no hits
    //     to mine.
    let prf_novel: Vec<Entity> = if config.prf_enabled && !fts_entities.is_empty() {
        let terms = prf::expansion_terms(
            &fts_entities[..fts_entities.len().min(config.prf_feedback_docs)],
            query,
            config.prf_max_terms,
        );
        if terms.is_empty() {
            Vec::new()
        } else {
            let expanded = prf::expand_query(query, &terms);
            let prf_entities = fts_search(
                conn,
                &expanded,
                type_filter,
                status_filter,
                !params.include_tests,
                limit,
                config.fts_title_weight,
            )?;
            let fts_ids: HashSet<&str> = fts_entities.iter().map(|e| e.id.as_str()).collect();
            let novel: Vec<Entity> = prf_entities
                .into_iter()
                .filter(|e| !fts_ids.contains(e.id.as_str()))
                .collect();
            for entity in &novel {
                entity_map.insert(entity.id.clone(), entity.clone());
            }
            novel
        }
    } else {
        Vec::new()
    };

    // 4c. Graph-first retrieval: traverse edges from FTS seeds to
    //     surface structurally related entities that embedding
    //     similarity can't reach. Candidates enter the merge below
    //     as a distinct channel — full RRF scores, not decayed
    //     expansion context. Runs in FTS-only mode too (no model
    //     dependency); bounded by FTS seed availability.
    let (graph_code_ids, graph_knowledge_ids, weak_graph) = if config.graph_first_enabled {
        // Seeds come from every direct channel: the top FTS hits plus
        // the top of each KNN space. A rule titled "Degradation must
        // be loud" is unreachable from "error handling rule" lexically
        // but a semantic KNN hit can still seed its references edges.
        // Each entity keeps its best per-channel rank weight.
        let denom = config.graph_max_seeds.max(1) as f64;
        let mut seed_weight: HashMap<String, f64> = HashMap::new();
        let mut direct_seed_ids: HashSet<String> = HashSet::new();
        fn add_ranked<'a, I: Iterator<Item = &'a String>>(
            seed_weight: &mut HashMap<String, f64>,
            denom: f64,
            max: usize,
            ids: I,
        ) {
            for (rank, id) in ids.take(max).enumerate() {
                let w = 1.0 / (1.0 + rank as f64 / denom);
                seed_weight
                    .entry(id.clone())
                    .and_modify(|s| {
                        if w > *s {
                            *s = w;
                        }
                    })
                    .or_insert(w);
            }
        }
        // KNN seeds carry a similarity floor: semantic neighbors are
        // noisier than lexical hits, and on lexically-aligned queries
        // weak KNN seeds pull in corroborated-but-irrelevant graph
        // candidates. Rather than dropping weak seeds (they carry real
        // recall on vocabulary-gap queries), they still traverse — but
        // candidates reachable ONLY through weak seeds are demoted to
        // the expansion set: they can inform, never displace. FTS
        // seeds are always direct-eligible (lexical match is the
        // reliable prior).
        let min_sim = config.graph_seed_min_sim;
        fn add_knn_seeds(
            seed_weight: &mut HashMap<String, f64>,
            direct_seed_ids: &mut HashSet<String>,
            denom: f64,
            max: usize,
            ids: &[String],
            dists: &[f32],
            min_sim: f64,
        ) {
            for (rank, (id, d)) in ids.iter().zip(dists.iter()).take(max).enumerate() {
                let w = 1.0 / (1.0 + rank as f64 / denom);
                seed_weight
                    .entry(id.clone())
                    .and_modify(|s| {
                        if w > *s {
                            *s = w;
                        }
                    })
                    .or_insert(w);
                if crate::embed::similarity::l2_to_cosine(*d as f64) >= min_sim {
                    direct_seed_ids.insert(id.clone());
                }
            }
        }
        add_ranked(
            &mut seed_weight,
            denom,
            config.graph_max_seeds,
            fts_entities[..direct_window].iter().map(|e| &e.id),
        );
        direct_seed_ids.extend(fts_entities[..direct_window].iter().map(|e| e.id.clone()));
        add_knn_seeds(
            &mut seed_weight,
            &mut direct_seed_ids,
            denom,
            config.graph_max_seeds,
            &code_ids,
            &code_distances,
            min_sim,
        );
        add_knn_seeds(
            &mut seed_weight,
            &mut direct_seed_ids,
            denom,
            config.graph_max_seeds,
            &knowledge_ids,
            &knowledge_distances,
            min_sim,
        );
        let mut seeds: Vec<(String, f64)> = seed_weight.into_iter().collect();
        seeds.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        seeds.truncate(config.graph_max_seeds * 3);

        // Only the direct window is excluded from graph candidacy —
        // deeper FTS hits are invisible to the direct merge, so graph
        // adjacency is exactly the signal that can still promote them.
        let exclude_ids: HashSet<String> = fts_entities[..direct_window]
            .iter()
            .map(|e| e.id.clone())
            .collect();
        let candidates = graph_retrieve(
            conn,
            &seeds,
            &direct_seed_ids,
            &exclude_ids,
            config.graph_max_hops,
            config.graph_hop_decay,
            params.limit as usize,
            status_filter,
            params.include_tests,
        )?;
        let uncached: Vec<String> = candidates
            .iter()
            .map(|c| c.entity_id.clone())
            .filter(|id| !entity_map.contains_key(id))
            .collect();
        for entity in get_entities_batch(conn, &uncached)? {
            entity_map.insert(entity.id.clone(), entity);
        }
        let mut code = Vec::new();
        let mut knowledge = Vec::new();
        let mut weak = Vec::new();
        for c in &candidates {
            let Some(entity) = entity_map.get(&c.entity_id) else {
                continue;
            };
            if !c.direct {
                weak.push((c.entity_id.clone(), c.score));
                continue;
            }
            if let Some(tf) = type_filter
                && entity.r#type != tf
            {
                continue;
            }
            match EntityType::parse(&entity.r#type) {
                Ok(t) if t.is_code() => code.push(c.entity_id.clone()),
                Ok(_) => knowledge.push(c.entity_id.clone()),
                Err(_) => {}
            }
        }
        (code, knowledge, weak)
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    deep_ids.extend(code_ids.iter().skip(params.limit as usize).cloned());
    deep_ids.extend(knowledge_ids.iter().skip(params.limit as usize).cloned());
    // Keep only deep candidates corroborated by at least two channels —
    // an entity ranked 25-60 in one channel is usually noise, but in
    // two channels it's a genuine miss the merge never saw. Without
    // this filter the deep pool floods the expansion cap.
    let mut counts: HashMap<String, usize> = HashMap::new();
    for id in &deep_ids {
        *counts.entry(id.clone()).or_default() += 1;
    }
    deep_ids.retain(|id| counts.get(id).copied().unwrap_or(0) >= 2);
    let mut code_ids = code_ids;
    code_ids.truncate(params.limit as usize);
    let mut knowledge_ids = knowledge_ids;
    knowledge_ids.truncate(params.limit as usize);

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
    //     match, the honest answer is an empty result — not a
    //     normalized list of garbage. Silence requires BOTH signals to
    //     be absent: flat within-batch gradient AND top-3 absolute
    //     strength below the floor. Gradient alone misfires on queries
    //     whose nearest neighbors are uniformly decent (flat spread,
    //     real matches) — strength is the check that nothing is close.
    //     FTS-only skips the gate: with no vector signal there is
    //     nothing to judge by.
    if params.silence_gate
        && let Some(s) = &signals
        && hybrid_helpers::should_silence(
            s,
            config.silence_threshold,
            config.silence_strength_floor,
        )
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
    let code_fused =
        if !code_fts_ids.is_empty() || !code_ids.is_empty() || !graph_code_ids.is_empty() {
            let mut lists: Vec<(&[String], f64)> = Vec::new();
            if !code_fts_ids.is_empty() {
                lists.push((&code_fts_ids, config.fts_weight));
            }
            if !code_ids.is_empty() {
                lists.push((&code_ids, config.code_vec_weight));
            }
            if !graph_code_ids.is_empty() {
                lists.push((&graph_code_ids, config.graph_weight));
            }
            fuse(&lists, config.rrf_k)
        } else {
            Vec::new()
        };

    let knowledge_fused = if !knowledge_fts_ids.is_empty()
        || !knowledge_ids.is_empty()
        || !graph_knowledge_ids.is_empty()
    {
        let mut lists: Vec<(&[String], f64)> = Vec::new();
        if !knowledge_fts_ids.is_empty() {
            lists.push((&knowledge_fts_ids, config.fts_weight));
        }
        if !knowledge_ids.is_empty() {
            lists.push((&knowledge_ids, config.vec_weight));
        }
        if !graph_knowledge_ids.is_empty() {
            lists.push((&graph_knowledge_ids, config.graph_weight));
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
    if !accessed_ids.is_empty()
        && let Err(e) = crate::storage::access::increment_access_batch(conn, &accessed_ids)
    {
        tracing::warn!("failed to increment access counts: {e}");
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
            None,
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

        // PRF second-pass hits join the expansion set: provenance is
        // "shares vocabulary with the top hit", scored like a 1-hop
        // expansion. They can win recall slots but never displace a
        // direct result.
        for entity in &prf_novel {
            if exclude_ids.contains(&entity.id) || !seen_expanded.insert(entity.id.clone()) {
                continue;
            }
            let seed_id = results[0].entity.id.clone();
            let decayed = seed_relevance.get(&seed_id).copied().unwrap_or(0.0) * 0.3;
            if decayed < config.min_relevance as f32 {
                filtered_count += 1;
                continue;
            }
            expanded_results.push(SearchResult {
                entity: entity.clone(),
                relevance: decayed,
                graph_path: vec![seed_id, entity.id.clone()],
                graph_path_description: "shares vocabulary with top hit".to_string(),
            });
        }

        // Deep-pool candidates — entities ranked just beyond `limit`
        // in the retrieval channels — join the expansion set. Recall
        // without direct-list displacement.
        let top_rel = results[0].relevance;
        for id in deep_ids {
            if exclude_ids.contains(&id) || !seen_expanded.insert(id.clone()) {
                continue;
            }
            let decayed = top_rel * 0.25;
            if decayed < config.min_relevance as f32 {
                filtered_count += 1;
                continue;
            }
            if let Some(entity) = entity_map.get(&id) {
                expanded_results.push(SearchResult {
                    entity: entity.clone(),
                    relevance: decayed,
                    graph_path: vec![results[0].entity.id.clone(), id.clone()],
                    graph_path_description: "deep candidate".to_string(),
                });
            }
        }

        // Graph candidates reachable only through weak KNN seeds —
        // evidence too thin for a direct slot, but real recall on
        // vocabulary-gap queries. Scored at parity with a 1-hop
        // expansion: graph adjacency is stronger evidence than deep
        // rank-overflow even when the seed is weak.
        for (id, score) in weak_graph {
            if exclude_ids.contains(&id) || !seen_expanded.insert(id.clone()) {
                continue;
            }
            // Clamp keeps ordering among weak candidates (corroborated
            // paths outrank lone ones) without letting the weakest
            // sink below the expansion pack's relevance band.
            let decayed = top_rel * 0.3 * (score.clamp(0.5, 1.0) as f32);
            if decayed < config.min_relevance as f32 {
                filtered_count += 1;
                continue;
            }
            if let Some(entity) = entity_map.get(&id) {
                expanded_results.push(SearchResult {
                    entity: entity.clone(),
                    relevance: decayed,
                    graph_path: vec![results[0].entity.id.clone(), id.clone()],
                    graph_path_description: "weak-seed graph candidate".to_string(),
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
