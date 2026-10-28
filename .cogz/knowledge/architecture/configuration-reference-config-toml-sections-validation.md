---
id: 6506448c-0cbd-4429-9124-f2e85dd8d1cc
title: Configuration reference — .cogz/config.toml sections and validation
type: knowledge
status: active
created_at: "2026-10-28T10:20:46Z"
updated_at: "2026-10-28T10:20:46Z"
references: ["0b0a90f1-166c-4397-b553-4b5a1b759dbf", "ac43ee1b-2322-4ba1-afcf-cd2464a2d065"]
category: architecture
tags: ["configuration", "config-toml", "validation", "knobs"]
verified_against: ["0b0a90f1-166c-4397-b553-4b5a1b759dbf=7a7ba965c62b93d9db6bdd6b9682784033fdc5049fb3547e2b4c4980a3b2da35", "ac43ee1b-2322-4ba1-afcf-cd2464a2d065=6d5439ce80de3225a8a7babcd69c146c18ba969718f834f08d46f415bab277d3"]
---

`.cogz/config.toml` — typed, startup-validated, versioned with the
project (canonical reference: `docs/configuration.md`).

**`[project]`** — `name` (autodetected from git remote/dir; metadata
only, never a filter).

**`[storage]`** — `db_path` must be relative and beneath `.cogz/`.

**`[embedding]`** — `code_model` (CodeRankEmbed-int8),
`knowledge_model` (bge-base), `dimension` (768 — must match both
models' registry dims, validated), `nli_model`
(nli-deberta-v3-xsmall), `auto_download` (true, opt-out),
`force_fts_only` (never load models — air-gap/RAM switch),
`code_embeddings` (false → skip the code model entirely; code
retrieval falls back to FTS + graph), `model_idle_ttl` (300 s),
`model_min_free_mb` (512). Available models are pinned in
`src/embed/registry.rs`; changing `dimension` requires swapping both
embedding models.

**`[search]`** — the biggest knob set. Fusion: `fts_weight` 0.3 /
`vec_weight` 0.4 / `code_vec_weight` 0.3 / `rrf_k` 60.
`merge_strategy`: `detect` (default, intent-proportioned), `fixed`,
`strength`, `gradient`, `calibrated` (best MRR, weaker graph recall).
Floors/gates: `min_relevance` 0.05 (≥0.09 kills 2-hop expansions),
`silence_coverage_floor` 0.45, `drift_penalty` 0.7 per diverged ref.
Ranking: `provenance_boost` 0.3 (curated in-degree prior),
`mmr_lambda` 0.7, `fts_title_weight` 5.0, `top_diversity_share` 0.2
(keep ≤ `min_source_proportion` or it never fires for floored
channels). Graph: `graph_first_enabled`, `graph_max_seeds` 20,
`graph_seed_min_sim` 0.7, `graph_max_hops` 2, `graph_hop_decay` 0.5,
`graph_weight` 0.35. Recall extras: `prf_enabled`,
`sibling_expand_enabled`, `cochange_enabled` (all default true).
`[search.calibration]` — per-channel `mid`/`width` for the
`calibrated` strategy, model-pair-specific.

**`[consolidation]`** — `dedup_threshold` 0.85,
`title_match_threshold` 0.85, `contradiction_check` true,
`promotion_threshold` 3, `contradiction_threshold` 0.70,
`contradiction_cosine_threshold` 0.85,
`contradiction_length_ratio` 5.0, `dedup_nli_threshold` 0.55
(either-direction entailment; see consolidation entity).

**`[index]`** — `allow`/`deny` globs (allow wins; patterns relative
to repo root), `max_file_bytes` 1 MiB (skipped files don't pin the
baseline), `split_identifiers` (populates `search_terms` FTS column
so "auth" hits `verifyAuthToken` — requires `cogz index` after
enabling).

**`[retention]`** — `observation_prune_after_days` 90,
`tombstone_max_count` 1000.

**`[context]`** — token budgets: `default` 4096, `task` 8192,
`escalation` 8192; `cold_start_rules` 5; `task_max_results` 40/
`task_max_hops` 2; `escalation_max_results` 20/`_hops` 3;
`tiered_push` true + `tier0_rules` 3.

**Startup validation rejects:** empty `project.name`, `db_path`
outside `.cogz/` or containing `..`, dimension/model mismatch,
`rrf_k`/`max_results`/budgets = 0, dedup thresholds outside [0,1],
`observation_prune_after_days` = 0. Invalid config exits before any
DB or file ops.
