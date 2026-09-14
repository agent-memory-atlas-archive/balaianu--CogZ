# Search

CogZ uses hybrid FTS5 + vector search with Reciprocal Rank Fusion (RRF) and graph expansion. This document describes the search pipeline.

## Pipeline

```
Query
  → embed query (knowledge model + code model, if available)
  → FTS5 search (always available)
  → KNN vector search (knowledge_embeddings + code_embeddings, if available)
  → RRF fusion (combine FTS + vector results)
  → Cross-encoder rerank (top `rerank_depth` directs, if model available)
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

## Cross-encoder rerank

After the top-N list is materialized and before graph expansion, an optional cross-encoder rescores the top `rerank_depth` (default 20) direct results (`rerank_enabled`, default true; `reranker_model`, default `cross-encoder/ms-marco-TinyBERT-L-2-v2`).

Unlike the bi-encoder stages, the cross-encoder reads each `(query, "title\ncontent")` pair jointly in a single batched inference — its sigmoid-calibrated probabilities are comparable across channels regardless of which embedding space retrieved the candidate. Inside the reranked window the merge-proportion mismatch dissolves, which is what lifts near-miss retrieval (expected entity in the candidate pool but outranked by semantically-adjacent ones) into the top-5. Entries beyond `rerank_depth` keep their fused order.

Two guards bound the failure mode of passage-domain cross-encoders, which systematically prefer prose over source code. `rerank_anchor` (default 3) pins the top fused positions outright. And unless `rerank_code` is enabled, code entities (function/class/file/module) hold their fused slots — they are excluded from scoring, so prose can never displace them. The remaining candidates reorder freely by cross-encoder score.

The stage runs before expansion so the reranked order propagates into expansion seeding. Expansion keeps the pre-rerank fused score as seed relevance — CE probabilities are near-binary, so seeding with them decays most expansions below the `min_relevance` floor. The displayed `relevance` of a reranked result is the CE probability. Missing model files or inference failure skip the stage — the fused list is still a valid answer. `rerank_enabled = false` disables it entirely.

## Relevance floor and silence gate

`min_relevance` (default: 0.05) drops merged and expanded results below the threshold; `filtered_count` in the response reports how many were removed. Note the decay interaction: any floor ≥ 0.09 makes all two-hop expansions unreachable (max expanded score is `seed × 0.3²`).

`silence_threshold` (default: 0.02) is the silence gate: when *neither* channel's KNN batch shows a distinctive match (within-batch gradient below the threshold on both), search returns empty instead of a confidently-ranked list of irrelevant entities. 0.0 disables. Skipped in FTS-only mode. The `signals` field in the response exposes both channels' strength and gradient for recalibration.

## Graph expansion

After RRF fusion, the top results are used as seeds for BFS graph expansion. The expansion follows all edge types (`references`, `auto_references`, `calls`, `imports`, `extends`, `contains`, `supports`, `derived_from`, `superseded_by`, `contradicts`).

With `edge_weighted_expansion` (default: true), edges are traversed strongest-first during BFS so curated semantic edges (`references`, `supports`, `contradicts`, `superseded_by`, `derived_from`, `promoted_from` = 1.0) claim contested nodes before `auto_references` (0.7) or structural edges like `imports`/`contains` (0.5). Expanded entities get a decayed relevance score: `seed_relevance × 0.3^hops × edge_weights`. This ensures direct matches rank higher than graph-expanded results and curated references survive the expansion cap instead of losing to mass structural fan-out.

Each result includes a `graph_path` — the list of entity IDs from the seed to this entity — and a human-readable `graph_path_description`.

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
