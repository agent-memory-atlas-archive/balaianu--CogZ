//! Post-merge ranking: channel weight resolution, relevance floor,
//! provenance prior, MMR diversification, and diversity slots.
//! Everything here operates on the fused (id, score, channel) list
//! after FTS/KNN fusion and before the final top-N cut.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::config::SearchConfig;
use crate::storage::StorageError;
use crate::storage::edges::curated_in_degree_batch;
use crate::storage::embeddings::{get_code_embeddings_batch, get_knowledge_embeddings_batch};
use crate::storage::query::{count_active_code, count_active_knowledge};

use super::SearchMode;
use super::balance::{
    channel_strength, detect_proportions, fts_pool_signal, mean_top_k_similarity,
};

/// Which fused channel an entity came from. Needed post-merge to
/// enforce the minority-channel slot guarantee and to scope MMR
/// similarity within a single embedding space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chan {
    Code,
    Knowledge,
}

/// One channel's retrieval signals for weight resolution: KNN
/// distances plus how many FTS and KNN candidates it produced.
pub struct ChannelInput<'a> {
    pub distances: &'a [f32],
    pub fts_count: usize,
    pub knn_count: usize,
}

/// Resolve per-channel merge weights for the configured strategy.
///
/// FTS-only mode keeps the neutral 50/50 split — without KNN
/// distances there is no absolute strength signal, so behavior must
/// stay identical. `detect` (and the legacy `source_balance_enabled`
/// flag) uses the proportion detector; `fixed` is the old quota;
/// `strength` scales each channel by absolute KNN match strength;
/// `gradient` uses within-batch distinctiveness; `calibrated`
/// multiplies detect shares by logistic-calibrated presence.
pub fn resolve_channel_weights(
    conn: &Connection,
    config: &SearchConfig,
    search_mode: SearchMode,
    code: &ChannelInput,
    knowledge: &ChannelInput,
) -> Result<(f64, f64), StorageError> {
    if search_mode == SearchMode::FtsOnly {
        return Ok((0.5, 0.5));
    }
    match config.merge_strategy.as_str() {
        _ if config.source_balance_enabled || config.merge_strategy == "detect" => {
            // Collection sizes normalize FTS match rates: code
            // entities vastly outnumber knowledge entities, so raw
            // match counts bias toward code.
            let code_size = count_active_code(conn)? as usize;
            let knowledge_size = count_active_knowledge(conn)? as usize;
            Ok(detect_proportions(
                code.distances,
                knowledge.distances,
                code.fts_count,
                knowledge.fts_count,
                code_size,
                knowledge_size,
                config.min_source_proportion,
            ))
        }
        "fixed" => Ok((0.5, 0.5)),
        "calibrated" => {
            // Detect decides the proportional split; logistic-
            // calibrated absolute strength decides whether each
            // channel deserves to surface at all. When a channel's
            // calibrated presence sinks, its whole list falls below
            // the relevance floor — calibrated silencing without
            // needing the gradient gate.
            let code_size = count_active_code(conn)? as usize;
            let knowledge_size = count_active_knowledge(conn)? as usize;
            let (mut cw, mut kw) = detect_proportions(
                code.distances,
                knowledge.distances,
                code.fts_count,
                knowledge.fts_count,
                code_size,
                knowledge_size,
                config.min_source_proportion,
            );
            if !code.distances.is_empty() {
                let c = &config.calibration.code;
                cw *= logistic(channel_strength(code.distances), c.mid, c.width);
            }
            if !knowledge.distances.is_empty() {
                let c = &config.calibration.knowledge;
                kw *= logistic(channel_strength(knowledge.distances), c.mid, c.width);
            }
            Ok((cw, kw))
        }
        "gradient" => {
            // Within-batch gradient as the unbounded weight — model-
            // agnostic: it measures whether this channel produced a
            // *distinctive* match, not how close it is on an
            // incomparable cosine scale. Flat KNN distances (nothing
            // stands out) → low weight → the relevance floor can
            // suppress the channel entirely.
            let fts_share = fts_pool_signal(code.fts_count, knowledge.fts_count, 0, 0);
            let code_has = code.fts_count > 0 || code.knn_count > 0;
            let knowledge_has = knowledge.fts_count > 0 || knowledge.knn_count > 0;
            let code_w = if !code_has {
                0.0
            } else if code.distances.is_empty() {
                fts_share * 0.5
            } else {
                mean_top_k_similarity(code.distances, 5)
            };
            let knowledge_w = if !knowledge_has {
                0.0
            } else if knowledge.distances.is_empty() {
                (1.0 - fts_share) * 0.5
            } else {
                mean_top_k_similarity(knowledge.distances, 5)
            };
            Ok((code_w, knowledge_w))
        }
        _ => {
            // "strength": a channel with FTS matches but no KNN
            // signal gets its sqrt-dampened pool share halved — no
            // absolute-confidence boost without an embedding
            // measurement.
            let fts_share = fts_pool_signal(code.fts_count, knowledge.fts_count, 0, 0);
            let code_has = code.fts_count > 0 || code.knn_count > 0;
            let knowledge_has = knowledge.fts_count > 0 || knowledge.knn_count > 0;
            let code_w = if !code_has {
                0.0
            } else if code.distances.is_empty() {
                fts_share * 0.5
            } else {
                channel_strength(code.distances)
            };
            let knowledge_w = if !knowledge_has {
                0.0
            } else if knowledge.distances.is_empty() {
                (1.0 - fts_share) * 0.5
            } else {
                channel_strength(knowledge.distances)
            };
            Ok((code_w, knowledge_w))
        }
    }
}

/// Run the post-merge ranking stages in order: provenance prior →
/// MMR diversification → minority-channel slot guarantee. Each stage
/// is independently disabled by its config knob.
pub fn apply_post_merge(
    conn: &Connection,
    fused: Vec<(String, f64, Chan)>,
    config: &SearchConfig,
    search_mode: SearchMode,
    code_prop: f64,
    knowledge_prop: f64,
    top_n: usize,
) -> Result<Vec<(String, f64, Chan)>, StorageError> {
    let mut fused = fused;

    // Provenance prior: entities that knowledge/rules deliberately
    // linked (curated edge in-degree) get a modest multiplicative
    // bump. Human-authored links are a quality signal the ranking
    // math can't see — this injects it.
    if config.provenance_boost > 0.0 && !fused.is_empty() {
        let ids: Vec<String> = fused.iter().map(|(id, _, _)| id.clone()).collect();
        let in_degrees = curated_in_degree_batch(conn, &ids)?;
        if !in_degrees.is_empty() {
            let b = config.provenance_boost;
            for (id, score, _) in fused.iter_mut() {
                let n = in_degrees.get(id).copied().unwrap_or(0);
                *score *= 1.0 + b * (1.0 + n as f64).ln();
            }
            fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }
    }

    // MMR diversification: near-duplicate entities (siblings in
    // the same file, overlapping observations) waste top slots —
    // rerank by λ·relevance − (1−λ)·max cosine to already-picked
    // items. Similarity only counts within a channel: code and
    // knowledge embeddings live in different spaces, and a cross-
    // channel pair is never a duplicate anyway.
    if config.mmr_lambda > 0.0 && fused.len() > 1 && search_mode != SearchMode::FtsOnly {
        let code_ids: Vec<String> = fused
            .iter()
            .filter(|(_, _, c)| *c == Chan::Code)
            .map(|(id, _, _)| id.clone())
            .collect();
        let knowledge_ids: Vec<String> = fused
            .iter()
            .filter(|(_, _, c)| *c == Chan::Knowledge)
            .map(|(id, _, _)| id.clone())
            .collect();
        let mut embeddings = get_code_embeddings_batch(conn, &code_ids)?;
        embeddings.extend(get_knowledge_embeddings_batch(conn, &knowledge_ids)?);
        fused = mmr_rerank(fused, &embeddings, config.mmr_lambda);
    }

    // Minority-channel slot guarantee: when the minority channel
    // still earned a meaningful share of the merge weight, its
    // best result is promoted into the top window if channel
    // concentration pushed it out. Recovers second-entity
    // coverage on mixed-intent queries without weakening the
    // winner's ordering.
    apply_diversity_slots(
        &mut fused,
        code_prop,
        knowledge_prop,
        config.top_diversity_share,
        top_n,
    );

    Ok(fused)
}

/// Merge normalized per-channel lists under per-channel weights, drop
/// entries below `min_relevance`, and return (survivors sorted desc,
/// number filtered). Weights need not sum to 1 — under the "strength"
/// strategy they are absolute match strengths, so a weak channel's
/// entire list sinks rather than filling a quota.
pub fn merge_channels(
    code: Vec<(String, f64)>,
    knowledge: Vec<(String, f64)>,
    code_weight: f64,
    knowledge_weight: f64,
    min_relevance: f64,
) -> (Vec<(String, f64, Chan)>, usize) {
    let mut fused: Vec<(String, f64, Chan)> = code
        .into_iter()
        .map(|(id, s)| (id, s * code_weight, Chan::Code))
        .chain(
            knowledge
                .into_iter()
                .map(|(id, s)| (id, s * knowledge_weight, Chan::Knowledge)),
        )
        .collect();
    let pre = fused.len();
    if min_relevance > 0.0 {
        fused.retain(|(_, s, _)| *s >= min_relevance);
    }
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let filtered = pre - fused.len();
    (fused, filtered)
}

/// Guarantee each channel whose weight share is at least `min_share`
/// a presence inside the top `window` positions (window = 5 or the
/// result limit, whichever is smaller). Reorders only — nothing is
/// removed. Minority channel is promoted first so that when both
/// qualify the under-weighted one wins the slot. `min_share` of 0
/// disables.
pub fn apply_diversity_slots(
    fused: &mut Vec<(String, f64, Chan)>,
    code_weight: f64,
    knowledge_weight: f64,
    min_share: f64,
    limit: usize,
) {
    if min_share <= 0.0 {
        return;
    }
    let total = code_weight + knowledge_weight;
    if total <= 0.0 {
        return;
    }
    let window = 5.min(limit).min(fused.len());
    if window == 0 {
        return;
    }
    let shares = |c: Chan| match c {
        Chan::Code => code_weight / total,
        Chan::Knowledge => knowledge_weight / total,
    };
    let minority_first = if code_weight <= knowledge_weight {
        [Chan::Code, Chan::Knowledge]
    } else {
        [Chan::Knowledge, Chan::Code]
    };
    for chan in minority_first {
        if shares(chan) < min_share {
            continue;
        }
        if fused.iter().take(window).any(|(_, _, c)| *c == chan) {
            continue;
        }
        if let Some(pos) = fused.iter().position(|(_, _, c)| *c == chan) {
            let item = fused.remove(pos);
            fused.insert(window - 1, item);
        }
    }
}

/// Greedy MMR rerank: each round picks the candidate maximizing
/// `λ·relevance − (1−λ)·max cosine similarity to already-selected
/// same-channel items`. Entities without embeddings get penalty 0 —
/// they compete on relevance alone. Runs before the diversity-slot
/// guarantee so the guarantee sees the final ordering.
fn mmr_rerank(
    mut candidates: Vec<(String, f64, Chan)>,
    embeddings: &HashMap<String, Vec<f32>>,
    lambda: f64,
) -> Vec<(String, f64, Chan)> {
    let mut selected: Vec<(String, f64, Chan)> = Vec::with_capacity(candidates.len());
    while !candidates.is_empty() {
        let mut best_i = 0;
        let mut best_v = f64::NEG_INFINITY;
        for (i, (id, score, chan)) in candidates.iter().enumerate() {
            let penalty = match embeddings.get(id) {
                Some(v) => selected
                    .iter()
                    .filter(|(_, _, c)| c == chan)
                    .filter_map(|(sid, _, _)| embeddings.get(sid))
                    .map(|sv| cosine_similarity(v, sv))
                    .fold(0.0f64, f64::max),
                None => 0.0,
            };
            let value = lambda * score - (1.0 - lambda) * penalty;
            if value > best_v {
                best_v = value;
                best_i = i;
            }
        }
        selected.push(candidates.remove(best_i));
    }
    selected
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Logistic calibration: maps a channel's absolute strength onto a
/// shared "probability a distinctive match exists" scale. `mid` is
/// the 50/50 point, `width` the transition sharpness.
fn logistic(strength: f64, mid: f64, width: f64) -> f64 {
    1.0 / (1.0 + (-(strength - mid) / width).exp())
}

/// Normalize RRF scores to [0, 1] range using max normalization.
/// The highest score becomes 1.0, others scale proportionally.
/// An empty list returns empty.
pub fn normalize_scores(scores: &[(String, f64)]) -> Vec<(String, f64)> {
    if scores.is_empty() {
        return Vec::new();
    }
    let max_score = scores.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);
    if max_score <= 0.0 {
        return scores.to_vec();
    }
    scores
        .iter()
        .map(|(id, s)| (id.clone(), s / max_score))
        .collect()
}

#[cfg(test)]
#[path = "rank_tests.rs"]
mod rank_tests;
