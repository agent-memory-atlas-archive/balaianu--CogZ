# Search

CogZ uses hybrid FTS5 + vector search with Reciprocal Rank Fusion (RRF) and graph expansion. This document describes the search pipeline.

## Pipeline

```
Query
  → embed query (knowledge model + code model, if available)
  → FTS5 search (always available)
  → KNN vector search (knowledge_embeddings + code_embeddings, if available)
  → Graph-first retrieval (BFS from FTS seeds → scored candidates)
  → RRF fusion (combine FTS + vector + graph results)
  → Graph expansion (BFS from matched entities)
  → Return ranked results with graph provenance
```

## FTS5 search

SQLite FTS5 with `porter unicode61` tokenizer. Searches the `entities_fts` virtual table, which is synced to the `entities` table via triggers (insert/update/delete).

FTS5 is always available, even without models. It provides lexical matching — exact term and phrase matches. It is the fallback when vector search is unavailable.

## Vector search

sqlite-vec provides KNN search over embedding vectors. Two separate vec0 tables:

- `code_embeddings` — embeddings from CodeRankEmbed (code entities)
- `knowledge_embeddings` — embeddings from bge-base (knowledge entities)

Code and knowledge use different embedding models with incompatible vector spaces even at the same dimensionality. Separate tables ensure KNN only compares vectors within the same space.

The query is embedded with both models (when available), producing two query embeddings. Each is used for a KNN search in its respective table.

## RRF fusion

Reciprocal Rank Fusion combines ranked lists from multiple sources without needing score calibration:

```
score(entity) = Σ  1 / (rrf_k + rank_in_source)
```

Where `rrf_k` is a smoothing constant (default: 60). Lower values produce sharper ranking.

**Sources and weights** (from `[search]` config):

| Source | Weight | When |
|---|---|---|
| FTS5 | `fts_weight` (0.3) | Always |
| Knowledge vector | `vec_weight` (0.4) | Knowledge model available |
| Code vector | `code_vec_weight` (0.3) | Code model available |

When a source is unavailable (no model), its weight is redistributed to the remaining sources.

## Channel merge strategies

`merge_strategy` controls how the normalized code and knowledge lists are weighted before merging:

- **`detect`** (default) — detects code vs knowledge query intent from two signals: KNN distance spread (a strong match produces a distance gradient; a weak match produces uniform distances) and FTS pool size ratio (a code query matches more code entities lexically). `min_source_proportion` (default: 0.2) keeps either source from being completely suppressed. Benchmark-validated: +27% MRR and higher graph recall over the fixed split.
- **`fixed`** — legacy 0.5/0.5 split. Available for A/B comparison and rollback.
- **`strength`** — scales each channel by absolute top-3 KNN cosine strength. Retained for experiments; absolute cosines are *not* comparable across the two embedding models (bge runs ~0.7, CodeRankEmbed ~0.45 on real matches), so this under-weights code.
- **`gradient`** — within-batch distinctiveness as an unbounded weight. Model-agnostic, but the code model's KNN distances cluster too tightly for it to register — suppresses the code channel.
- **`calibrated`** — detect shares × per-channel logistic-calibrated absolute strength (`search.calibration`). Proportion decides the split; calibrated presence decides whether a channel surfaces at all. Best MRR measured, but suppressing weak channels starves graph-expansion seeds — lower graph recall than `detect`.

`source_balance_enabled = true` is a legacy alias for `detect`.

`top_diversity_share` (default: 0.3) is a slot guarantee layered on top: a channel earning at least that share of the merge weight gets its best result promoted into the top-5 window if ranking pushed it out. It recovers mixed-intent coverage that channel concentration loses, at no cost to the ordering above the window.

`fts_title_weight` (default 5.0) biases the FTS stage itself: `bm25(entities_fts, title_weight, 1.0)` exploits the FTS table's separate `title`/`content` columns so exact-name hits outrank body-term matches without a second index.

## Post-merge ranking signals

Two optional stages run between channel merge and the final top-N cut, in this order (see `src/search/rank.rs`):

1. **Provenance prior** (`provenance_boost`, default 0.3): `score ×= 1 + boost·ln(1 + curated_in_degree)` where the in-degree counts only incoming *curated* edge types (`references`, `supports`, `contradicts`, `superseded_by`, `derived_from`, `promoted_from`) — `auto_references` and structural edges are excluded because generated links carry no human judgment. One batched `GROUP BY target_id` query via `curated_in_degree_batch` in `src/storage/edges.rs`. Benchmark: graph R@20 0.72→1.0, MRR +26%.
2. **MMR diversification** (`mmr_lambda`, default 0.7): greedy rerank by `λ·relevance − (1−λ)·max cosine to already-selected` items. Similarity is computed only between same-channel candidates — code and knowledge embeddings live in different spaces, and a cross-channel pair is never a duplicate. Embeddings are fetched in two batched queries (`get_code_embeddings_batch`, `get_knowledge_embeddings_batch`); entities without embeddings get penalty 0. Skipped in FTS-only mode.

The diversity-slot guarantee (`top_diversity_share`, described above) runs last, on the post-MMR ordering.

## Relevance floor and silence gate

`min_relevance` (default: 0.05) drops merged and expanded results below the threshold; `filtered_count` in the response reports how many were removed. Note the decay interaction: any floor ≥ 0.09 makes all two-hop expansions unreachable (max expanded score is `seed × 0.3²`).

`silence_threshold` (default: 0.02) is the silence gate: when *neither* channel's KNN batch shows a distinctive match (within-batch gradient below the threshold on both) **and** neither channel's top-3 absolute cosine reaches `silence_strength_floor` (default: 0.64), search returns empty instead of a confidently-ranked list of irrelevant entities. The strength escape matters: flat gradients also occur when a query's nearest neighbors are uniformly *decent* (no standout), which suppressed real queries entirely. 0.0 disables. Skipped in FTS-only mode. The `signals` field in the response exposes both channels' strength and gradient for recalibration.

## Graph-first retrieval

With `graph_first_enabled` (default: true), the top `graph_max_seeds` (default: 20) hits of **each direct channel** — FTS, code KNN, and knowledge KNN — seed a bounded BFS before fusion over **curated edge types only** (`references`, `supports`, `contradicts`, `superseded_by`, `derived_from`, `promoted_from`). Multi-channel seeding matters because a lexical miss can still be a semantic hit: a rule titled "Degradation must be loud" is unreachable from "error handling rule" via FTS but a KNN top hit can still seed its `references` edges. Seeds are capped at `graph_max_seeds × 3` total after per-channel rank-weighting.

Traversal runs **per seed** (the shared visited set in `expand_with_paths` would otherwise claim each entity for its first-reaching seed and hide corroboration), and a candidate's score is the **sum over all (seed, path) contributions**: `seed_weight × graph_hop_decay^hops × edge_weights`. Summed reinforcement is the selectivity signal that lets wide seeding add recall — entities adjacent to several seeds outrank single-seed noise. Candidates below 0.2 are dropped (a lone weak path can't outrank real evidence but would still occupy RRF ranks). `graph_hop_decay` defaults to 0.5 (vs. the 0.3 expansion decay — these are primary candidates, not context).

KNN seeds are additionally gated by `graph_seed_min_sim` (default: 0.7 cosine): a KNN hit below the floor still traverses, but a candidate reachable **only** through sub-floor seeds is marked weak and demoted to the post-merge expansion set — it can add recall but never occupy a direct slot. FTS seeds are always direct-eligible (a lexical match is a reliable prior). This bounds the graph channel's noise on lexically-aligned queries, where weak semantic neighbors otherwise pull in corroborated-but-irrelevant candidates. Candidates split by entity type and fuse into the code and knowledge channels as a third list weighted `graph_weight` (default: 0.35).

Structural edges (`contains`, `calls`, `imports`) are deliberately excluded from this channel — measured on the benchmark, their fan-out injected more noise than signal and displaced correct direct results. `auto_references` (auto-extracted knowledge→code mentions) were tested and also excluded: reachable to real misses but not selective enough for direct slots. Both remain in the post-merge expansion path. In FTS-only mode seeds come from FTS alone. Set `graph_first_enabled = false` for the legacy pipeline.

## Graph expansion

After RRF fusion, the top results are used as seeds for BFS graph expansion. The expansion follows all edge types (`references`, `auto_references`, `calls`, `imports`, `extends`, `contains`, `supports`, `derived_from`, `superseded_by`, `contradicts`).

With `edge_weighted_expansion` (default: true), edges are traversed strongest-first during BFS so curated semantic edges (`references`, `supports`, `contradicts`, `superseded_by`, `derived_from`, `promoted_from` = 1.0) claim contested nodes before `auto_references` (0.7) or structural edges like `imports`/`contains` (0.5). Expanded entities get a decayed relevance score: `seed_relevance × 0.3^hops × edge_weights`. This ensures direct matches rank higher than graph-expanded results and curated references survive the expansion cap instead of losing to mass structural fan-out.

Each result includes a `graph_path` — the list of entity IDs from the seed to this entity — and a human-readable `graph_path_description`.

Two non-edge sources also join the expansion set, scored like 1-hop expansions and subject to the same cap:

- **PRF second pass** (`prf_enabled`, default: true): informative terms mined from the top `prf_feedback_docs` (5) FTS hits — appearing in at least two of them, title-weighted — extend the query for a second FTS pass (`prf_max_terms`, 8). Hits not already in the first-pass results join as "shares vocabulary" expansions. This is the only vocabulary-mismatch recall path when embedding models are absent.
- **Deep-channel candidates**: channels fetch `limit × 3` and entities ranked beyond `limit` in *at least two* channels join as "deep candidate" expansions. Single-channel deep hits are usually noise; multi-channel corroboration is the selectivity test.

**Max hops** is configurable per mode:
- Task mode: `task_max_hops` (default: 2)
- Escalation mode: `escalation_max_hops` (default: 3)

Use `--no-expand` on the CLI or `expand: false` in the MCP tool to disable expansion.

## Search modes

The `search_mode` field in results indicates how search was executed:

| Mode | FTS | Knowledge vector | Code vector | When |
|---|---|---|---|---|
| `hybrid` | yes | yes | yes | Both models available |
| `knowledge_hybrid` | yes | yes | no | Code model unavailable |
| `code_hybrid` | yes | no | yes | Knowledge model unavailable |
| `fts_only` | yes | no | no | No models available |

## Code search

The `--code` flag (CLI) or `code_search: true` (MCP) uses the CodeRankEmbed model for query embedding instead of the knowledge model. CodeRankEmbed requires a query prefix: `"Represent this query for searching relevant code: "` prepended to the query. This is applied automatically.

Use `--code` for queries about code structure, function behavior, or implementation details. Use the default (knowledge model) for queries about concepts, decisions, or documentation.

## Known limitations

1. **Graph expansion dilutes precision.** Graph-expanded entities are technically related but may not contain the query keywords. The decayed relevance score, edge weighting, and `min_relevance` floor mitigate this but don't eliminate it.
2. **Code/knowledge embedding mismatch.** When a code query is embedded with the knowledge model (or vice versa), the KNN search may miss relevant results. Using `--code` for code-focused queries helps.
3. **Silence gate margin is thin.** The default `silence_threshold` (0.02) was calibrated on the CogZ self-corpus where negatives measured ≤0.015 and real queries ≥0.024. Other corpora or models may straddle it — check `signals` and retune if needed.

## See also

- [Architecture](architecture.md) — system overview
- [Configuration](../configuration.md) — search config section
- [Degradation](degradation.md) — FTS-only mode
- [Evaluations](../evaluations/) — retrieval benchmarks and resource profiles
