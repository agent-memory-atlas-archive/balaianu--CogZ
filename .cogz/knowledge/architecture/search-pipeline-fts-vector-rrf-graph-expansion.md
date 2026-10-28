---
id: ac43ee1b-2322-4ba1-afcf-cd2464a2d065
title: "Search pipeline — FTS, vector, RRF, graph expansion"
type: knowledge
status: active
created_at: "2026-08-28T19:22:00Z"
updated_at: "2026-10-28T10:20:46Z"
references: ["a9d8f4cd-a22a-4e0c-a25a-418c92564dcd"]
category: architecture
tags: ["search", "fts5", "vector", "rrf", "graph-expansion"]
verified_against: ["a9d8f4cd-a22a-4e0c-a25a-418c92564dcd=3145d032b6d1c1836af620ad7d3c5b8d852477de0c76889f2cadf8c2423a502c"]
---

The search pipeline in `src/search/hybrid.rs` (canonical doc:
`docs/design/search.md`):

1. **Query embedding** — knowledge model always; CodeRankEmbed for
   `--code` queries (auto-prepends the required
   `"Represent this query for searching relevant code: "` prefix).

2. **Direct channels** — FTS5 (`porter unicode61`, title column
   weighted `fts_title_weight` = 5.0 in `bm25()`), then KNN over the
   two vec0 tables (`code_embeddings`, `knowledge_embeddings` —
   separate because the models' spaces are incomparable). KNN
   over-fetches `limit × 3`; post-KNN type/status filtering happens
   in Rust because vec0 can't JOIN `entities`.

3. **Graph-first retrieval** — top `graph_max_seeds` (20) hits per
   direct channel seed a bounded BFS over **curated edges only**
   (`references`, `supports`, `contradicts`, `superseded_by`,
   `derived_from`, `promoted_from` — structural and `auto_references`
   measured too noisy for direct slots). Per-seed traversal; score =
   Σ over paths of `seed_weight × graph_hop_decay^hops ×
   edge_weights` — multi-seed corroboration is the selectivity
   signal; candidates < 0.2 dropped. KNN seeds gated by
   `graph_seed_min_sim` (0.7): sub-floor seeds still traverse but
   their exclusive candidates demote to expansion. Fuses as a third
   channel weighted `graph_weight` (0.35).

4. **Channel merge** — `merge_strategy` (default `detect`:
   intent-proportioned shares from KNN distance spread + FTS pool
   ratio, floored at `min_source_proportion` 0.2; `calibrated` =
   best MRR but starves graph seeds). RRF: `Σ 1/(rrf_k + rank)` per
   weighted list.

5. **Post-merge ranking** — in order: drift demotion
   (`drift_penalty` 0.7^drift_count from `entity_drift`), provenance
   prior (`provenance_boost` 0.3, curated in-degree only),
   MMR diversification (`mmr_lambda` 0.7, same-channel only), then
   `top_diversity_share` window quota + top-5 guarantee.
   `min_relevance` 0.05 floors everything.

6. **Silence gate** — agent-facing search only (packs bypass it):
   `silence_coverage_floor` 0.45 on IDF-weighted query-term coverage
   in the fused top-10. Coverage, not embedding strength — corpus-
   absent terms at max IDF are the "no answer here" signal. Runs in
   FTS-only mode. ~60% of unanswerable queries silenced at ~7%
   false-silence cost.

7. **Expansion** — post-merge BFS over ALL edge types, scored
   `seed × 0.3^hops × edge_weights` (curated 1.0 > auto_references
   0.7 > structural 0.5), strongest-first traversal. Non-edge
   channels join as 1-hop-scored candidates: file siblings
   (`sibling_expand_enabled`), git co-change (`cochange_enabled`,
   mined at index from libgit2 commit diffs), PRF second FTS pass
   (`prf_enabled` — the only vocabulary-mismatch recall in FTS-only
   mode), deep-channel candidates (in ≥2 channels' over-fetched
   tails). Results carry `graph_path` + description.

`search_mode` reports `hybrid` / `knowledge_hybrid` / `code_hybrid`
/ `fts_only` per which models loaded. `SearchConfig.max_results`
(20) is the default limit. `--no-expand` / `expand: false` skips
step 7.
