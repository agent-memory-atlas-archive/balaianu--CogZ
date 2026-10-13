---
id: 9f2a1c3e-7b44-4a5f-9c6d-2e8f1a4b7c90
title: "Retrieval recall investigation: silence gate was the dominant miss source"
type: knowledge
status: active
created_at: "2026-09-15T14:55:00+00:00"
updated_at: "2026-09-15T14:55:00+00:00"
references: []
category: decisions
tags: ["decision", "search", "recall", "silence-gate", "prf", "benchmark"]
---

# Retrieval recall investigation (2026-09-15)

Failure decomposition on v2 benchmark (117 expected entities, self-corpus):
~50% of expected entities never surfaced. Root causes in order of impact:

1. **Silence gate misfire (dominant).** The gradient-only gate silenced ~15
   real queries whose top-5 KNN batches were uniformly *decent* (flat spread,
   no standout). Adding `silence_strength_floor` (top-3 absolute cosine ≥0.64
   escapes) recovered them: v2 MRR 0.366→0.416 alone. Cost: 1 v2 negative
   leaked (v2n04 "postgresql pool" → `open` at 0.66 strength — a genuine
   semantic collision, not signal noise).

2. **Fixed top-20 substitution.** Graph candidates, PRF hits, and deep-pool
   entities all substitute inside capped lists — adding a channel cannot grow
   recall, only displace. This is why graph-first's R@20 stayed flat.

3. **Edge selectivity.** Curated edges (`references` etc., ~95 edges) are
   precise but sparse. `auto_references` (652 edges) reach many misses but
   inject unselective mention noise — measured net-negative as a direct
   channel. Structural edges (contains/imports/calls, ~3000 edges) are pure
   fan-out noise in the direct channel.

## What shipped

- graph-first channel: curated edges only, weight 0.35 — wins on relational
  queries (v2 g01 0.05→1.0) at ~3-4 displaced answers.
- **Multi-channel seeding**: seeds = top `graph_max_seeds` (20) of FTS AND
  both KNN spaces. A lexical miss can still be a semantic hit — "error
  handling rule" can't reach "Degradation must be loud" via FTS but KNN
  seeds its `references` edges (v2g01: 0→all 3 expected entities direct).
- **Per-seed traversal + summed scoring** (PPR-lite): expand_with_paths'
  global visited set claims each entity for its first-reaching seed, hiding
  corroboration. Per-seed traversal lets every (entity, seed) pair
  contribute; multi-seed entities reinforce and outrank single-seed noise.
  This is what made wide seeding viable: v2 graph MRR 0.098→0.429.
- Candidate score floor 0.2 in the graph channel — lone weak paths can't
  occupy RRF ranks.
- Exclusion fix: graph candidates exclude only the direct FTS window, not
  the whole 60-deep fetch — deep FTS hits can be promoted by graph
  adjacency (they're invisible to the direct merge otherwise).
- PRF: expansion-set only (never displaces). Zero measurable hybrid-mode
  benefit; kept as the only vocabulary-mismatch recall path in FTS-only mode.
- Deep-channel corroborated candidates (rank ≥ limit in ≥2 channels) join the
  expansion set. Wide-fetch-direct (all 60 in channels) measured: R@20 0.734
  but MRR 0.364 — rejected, noise won direct slots.
- Silence gate: `flat-gradient AND strength<floor` required for silence.

## Final numbers (v2, 78q / v1, 32q)

- v2: MRR 0.412 (base 0.367), P@5 0.121 (0.106), R@20 0.703 (0.581),
  graph MRR 0.429 (0.16), neg_ok 0.6, ~450ms (was 386ms)
- v1: MRR 0.624 (base 0.702 — regression), R@20 0.929 (0.964),
  graph MRR 0.667 (0.35), neg_ok 1.0, pack recall 1.0
- v1 regression is concentrated displacement on lexically-aligned queries
  where the graph channel is redundant-but-noisy. Residual belongs to
  intent-aware routing (Phase 3): gate graph weight when the query has no
  relational cue.

## What did NOT work (measured, don't re-attempt)

- auto_references/structural edges in the direct channel (fan-out noise)
- PRF as a fuse channel at any weight (reorders shared hits, displaces)
- Lowering graph_weight to 0.15 (gives up the wins without recovering losses)
- Wide direct fetch (recall gain is real but MRR cost is worse)
