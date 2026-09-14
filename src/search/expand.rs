//! Graph-aware expansion from search results.
//!
//! For each seed entity (a direct search match), follows edges outward
//! via BFS, recording the path from the seed to each discovered entity.
//! This provides provenance: the agent can see *why* an entity was
//! included in the results.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::storage::StorageError;
use crate::storage::graph::get_edges_involving_batch;

/// Edge-type quality weight, shared by claim ordering and scoring.
/// Curated semantic edges — a human wrote `references:` in frontmatter —
/// outrank mass structural fan-out, so explicit links both claim
/// contested nodes during BFS and survive the expansion result cap.
pub(crate) fn edge_weight(edge_type: &str) -> f32 {
    match edge_type {
        "references" | "supports" | "contradicts" | "superseded_by" | "derived_from"
        | "promoted_from" => 1.0,
        "auto_references" => 0.7,
        _ => 0.5,
    }
}

/// A graph-expanded entity with its provenance path.
#[derive(Debug, Clone)]
pub struct ExpansionResult {
    pub entity_id: String,
    /// Path from the seed to this entity: [seed_id, hop1, hop2, ..., this_id]
    pub graph_path: Vec<String>,
    /// Edge types along `graph_path`: edge_path[i] connects
    /// graph_path[i] → graph_path[i+1]. Parallel to graph_path minus
    /// the seed element.
    pub edge_path: Vec<String>,
    /// Which seed entity this was expanded from.
    pub seed_id: String,
}

/// Expand from seed entities via fused multi-seed BFS, recording paths.
///
/// All seeds are placed in the initial frontier and expanded together.
/// This means one `get_edges_involving_batch` query per hop instead of
/// one per hop per seed. Each discovered entity records which seed it
/// was reached from and the path from that seed.
///
/// Follows both outgoing and incoming edges. Entities already in
/// `exclude_ids` are not returned (prevents duplicating direct search
/// matches). Only entities matching `status_filter` are included.
pub fn expand_with_paths(
    conn: &Connection,
    seed_ids: &[String],
    max_hops: usize,
    exclude_ids: &HashSet<String>,
    status_filter: Option<&str>,
    include_tests: bool,
) -> Result<Vec<ExpansionResult>, StorageError> {
    if max_hops == 0 || seed_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Global visited set across all seeds — an entity discovered from
    // one seed is not rediscovered from another.
    let mut visited: HashSet<String> = HashSet::new();
    for seed_id in seed_ids {
        visited.insert(seed_id.clone());
    }

    // Map entity_id → (node path from seed, edge-type path, seed_id)
    let mut paths: HashMap<String, (Vec<String>, Vec<String>, String)> = HashMap::new();
    for seed_id in seed_ids {
        paths.insert(
            seed_id.clone(),
            (vec![seed_id.clone()], Vec::new(), seed_id.clone()),
        );
    }

    // Initial frontier: all seeds
    let mut frontier: Vec<String> = seed_ids.to_vec();
    let mut discovered = Vec::new();

    for _ in 0..max_hops {
        if frontier.is_empty() {
            break;
        }

        // Single query: all edges where either endpoint is in the frontier
        let mut edges = get_edges_involving_batch(conn, &frontier)?;
        if edges.is_empty() {
            break;
        }

        // First discovery claims a node. Sort so higher-quality edges
        // claim contested nodes: without this, a curated `references`
        // edge loses the claim race to whatever `auto_references` edge
        // happens to come first in table order, and the result is scored
        // from the weaker seed + weaker edge type.
        edges.sort_by(|a, b| {
            edge_weight(&b.2)
                .partial_cmp(&edge_weight(&a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let frontier_set: HashSet<&String> = frontier.iter().collect();
        let mut next_neighbors: Vec<String> = Vec::new();
        let mut candidates: Vec<(String, Vec<String>, Vec<String>, String)> = Vec::new();

        for (source, target, edge_type) in &edges {
            let (parent, neighbor) = if frontier_set.contains(source) {
                (source, target)
            } else if frontier_set.contains(target) {
                (target, source)
            } else {
                continue;
            };

            if visited.insert(neighbor.clone()) {
                let (parent_path, parent_edges, seed_id) = paths
                    .get(parent)
                    .cloned()
                    .ok_or(StorageError::EntityNotFound(parent.clone()))?;
                let mut path = parent_path;
                path.push(neighbor.clone());
                let mut epath = parent_edges;
                epath.push(edge_type.clone());
                paths.insert(
                    neighbor.clone(),
                    (path.clone(), epath.clone(), seed_id.clone()),
                );

                if !exclude_ids.contains(neighbor) {
                    candidates.push((neighbor.clone(), path, epath, seed_id));
                }

                next_neighbors.push(neighbor.clone());
            }
        }

        // Batch status check: one query for all candidates in this hop
        if !candidates.is_empty() {
            // Filter by status first
            let status_filtered: Vec<(String, Vec<String>, Vec<String>, String)> =
                match status_filter {
                    None | Some("all") => candidates,
                    Some(status) => {
                        let ids: Vec<String> =
                            candidates.iter().map(|(id, _, _, _)| id.clone()).collect();
                        let matching = batch_check_status(conn, &ids, status)?;
                        let include_set: HashSet<&String> = matching.iter().collect();
                        candidates
                            .into_iter()
                            .filter(|(id, _, _, _)| include_set.contains(id))
                            .collect()
                    }
                };

            // Filter out test code entities when include_tests is false.
            // Uses the shared language-aware is_test_file function for
            // consistent test detection across all supported languages.
            let final_candidates = if include_tests {
                status_filtered
            } else {
                let ids: Vec<String> = status_filtered
                    .iter()
                    .map(|(id, _, _, _)| id.clone())
                    .collect();
                let test_ids = batch_check_test_paths_rust(conn, &ids)?;
                let test_set: HashSet<&String> = test_ids.iter().collect();
                status_filtered
                    .into_iter()
                    .filter(|(id, _, _, _)| !test_set.contains(id))
                    .collect()
            };

            for (id, path, epath, seed_id) in final_candidates {
                discovered.push(ExpansionResult {
                    entity_id: id,
                    graph_path: path,
                    edge_path: epath,
                    seed_id,
                });
            }
        }

        frontier = next_neighbors;
    }

    Ok(discovered)
}

/// Batch-check which entity IDs have the given status. Returns the
/// subset of `ids` whose status matches. Chunks the query to respect
/// SQLite's variable number limit (one extra var for status).
fn batch_check_status(
    conn: &Connection,
    ids: &[String],
    status: &str,
) -> Result<Vec<String>, StorageError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // SQLITE_MAX_VARIABLE_NUMBER is 999 by default. Each id is one var,
    // plus one for the status parameter → chunk at 998.
    const MAX_VARS: usize = 999;
    const CHUNK_SIZE: usize = MAX_VARS - 1; // 998

    let mut matching = Vec::new();

    for chunk in ids.chunks(CHUNK_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let mut params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        params.push(&status);
        let sql = format!("SELECT id FROM entities WHERE id IN ({placeholders}) AND status = ?");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0))?;
        for row in rows {
            matching.push(row?);
        }
    }

    Ok(matching)
}

/// Batch-check which entity IDs have test file paths. Fetches
/// file_path for each ID in a single batched query, then filters
/// using the shared language-aware `is_test_file` function.
fn batch_check_test_paths_rust(
    conn: &Connection,
    ids: &[String],
) -> Result<Vec<String>, StorageError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    use crate::index::is_test_file;

    const CHUNK_SIZE: usize = 998;

    let mut matching = Vec::new();

    for chunk in ids.chunks(CHUNK_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let sql = format!("SELECT id, file_path FROM entities WHERE id IN ({placeholders})");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        for row in rows {
            let (id, file_path) = row?;
            if let Some(ref fp) = file_path
                && is_test_file(fp)
            {
                matching.push(id);
            }
        }
    }

    Ok(matching)
}

#[cfg(test)]
#[path = "expand_tests.rs"]
mod tests;
