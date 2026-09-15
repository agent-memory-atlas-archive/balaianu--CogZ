//! Graph-first retrieval — FTS seeds → edge traversal → scored
//! candidates that enter the merge as a distinct channel.
//!
//! Embedding similarity can't surface the function that loads the
//! inference library when nothing names it "inference" — but the call
//! graph knows exactly which functions call into the loading path.
//! Unlike post-merge expansion (decayed context), these are primary
//! candidates competing with FTS and KNN on equal footing.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::SearchError;
use super::expand::{CURATED_EDGE_TYPES, edge_weight, expand_with_paths};

/// A scored graph candidate. `direct` is true when at least one
/// direct-eligible seed (FTS hits, or KNN hits above the similarity
/// floor) reached it — weak-seed-only candidates are recall context,
/// not primary evidence.
pub struct GraphCandidate {
    pub entity_id: String,
    pub score: f64,
    pub direct: bool,
}

/// Traverse graph edges outward from weighted seeds and return scored
/// candidate entity IDs, best first.
///
/// Seeds are `(id, weight)` pairs — the caller merges FTS and KNN
/// channel tops so semantic matches can seed traversal even when they
/// have no lexical overlap with the query. A candidate at `h` hops
/// scores `seed_weight × hop_decay^h × ∏ edge_weight` — less aggressive
/// than expansion decay because these compete as primary results, not
/// context. Entities in `exclude_ids` (the direct-merge window) are
/// skipped as candidates; traversal still passes through them.
#[allow(clippy::too_many_arguments)]
pub fn graph_retrieve(
    conn: &Connection,
    seeds: &[(String, f64)],
    direct_seeds: &HashSet<String>,
    exclude_ids: &HashSet<String>,
    max_hops: usize,
    hop_decay: f64,
    max_candidates: usize,
    status_filter: Option<&str>,
    include_tests: bool,
) -> Result<Vec<GraphCandidate>, SearchError> {
    if seeds.is_empty() || max_hops == 0 || max_candidates == 0 {
        return Ok(Vec::new());
    }
    // Curated edges only: a human wrote these links, so a neighbor is
    // a precision signal. `auto_references` was measured to inject
    // more noise than signal — knowledge files mention many functions,
    // so mention edges lack selectivity for direct-result slots.
    let curated: HashSet<&str> = CURATED_EDGE_TYPES.iter().copied().collect();

    // Per-seed traversal: expand_with_paths uses a global visited set
    // that claims each entity for its first-reaching seed, hiding
    // corroboration. Traversing per seed lets every (entity, seed)
    // pair contribute — multi-seed entities reinforce, single-seed
    // neighbors stay weak.
    let mut best: HashMap<String, (f64, bool)> = HashMap::new();
    for (seed_id, seed_w) in seeds {
        let direct_seed = direct_seeds.contains(seed_id);
        let expansions = expand_with_paths(
            conn,
            std::slice::from_ref(seed_id),
            max_hops,
            exclude_ids,
            status_filter,
            include_tests,
            Some(&curated),
        )?;
        for exp in expansions {
            let hops = exp.graph_path.len().saturating_sub(1);
            let edge_w: f64 = exp
                .edge_path
                .iter()
                .map(|t| f64::from(edge_weight(t)))
                .product();
            let score = seed_w * hop_decay.powi(hops as i32) * edge_w;
            best.entry(exp.entity_id)
                .and_modify(|(s, d)| {
                    *s += score;
                    *d |= direct_seed;
                })
                .or_insert((score, direct_seed));
        }
    }

    let mut candidates: Vec<GraphCandidate> = best
        .into_iter()
        .map(|(entity_id, (score, direct))| GraphCandidate {
            entity_id,
            score,
            direct,
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Score floor: a candidate needs at least one strong path or
    // several weak ones. Single 2-hop paths and weakest-seed
    // 1-hop neighbors (~<0.2) can't outrank real evidence but would
    // still occupy RRF ranks in the channel.
    candidates.retain(|c| c.score >= 0.2);
    // Direct and weak-only candidates cap separately — the direct cap
    // bounds the merge channel, the weak cap bounds expansion recall.
    let mut direct = Vec::with_capacity(max_candidates);
    let mut weak = Vec::with_capacity(max_candidates);
    for c in candidates {
        if c.direct && direct.len() < max_candidates {
            direct.push(c);
        } else if !c.direct && weak.len() < max_candidates {
            weak.push(c);
        }
    }
    direct.extend(weak);
    Ok(direct)
}

#[cfg(test)]
#[path = "graph_retrieval_tests.rs"]
mod tests;
