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
verified_against: ["034336be-ef90-55fb-8084-fc05f25701f3=30ea039cbd7cf18fd7b6d05c25bc890d6243193936d5e127fdf88dba08911b33", "047656c5-e449-58d7-b951-3b948a609cd9=2573200a85d75170d672883dfed5d39c5e19f3c4d15c0dcb8d3c5e1aa82bfe59", "16cf4100-a6e8-5c76-9f1e-4b4b3cb289f4=e458fa2c45119c41c3a88ff5392136a84092b35eb452a469f24611e959e2af1f", "2d301065-3f1b-5d81-937e-9972bf96faef=e836c44e68b157bf561636ecdeeafbcb50eaed075370f3bc687439d10ae82144", "39481ecc-d279-5278-831e-559ddcd736ce=b945b261c6197d64d8456435a7ed7aaf07e23df471aba7e7069f58c2b434d842", "3e1560ff-3053-529a-a434-55ee043a760f=ca06a9e4230916af7e6e53677439a32ff463394cd6ad8ce013b1bfae119498ee", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "4e0d412d-43f2-57db-8df0-788e0e21eb8d=5d65fec322c809af1d7b153866e3550c663711591edd7065f08261eabf2790c2", "524f7d8f-9d30-5a5d-ab62-6a4472fa555b=7fca08c8e9e2fad1c942dba41ea47264f0f8bca7d23c047ec39f3d45ac726ca6", "6c13bd51-5ca8-532a-8828-20c95debe650=16962e335c0871f99f5002be9b08184091285e451f4de92f1ea0d37252e24bfb", "793f6448-0e2e-5da3-83d2-e7a560d240fd=acda2bbb404d45d5dd4f15d1b10d4e9573176ec3290656e2a2f4b55e71c97dac", "8677ca42-fa21-5c89-bb0a-dba2e79d7766=0dd47f35b18b2db771d4f308351dc6f0000ceb9e479a539eb85bc77a8d574759", "90898141-d978-5ef9-bcdd-5ccd4c821a8f=59c1a41048a54322722f1c72b7c3b8a310001870c880eb8d54b7e369e6601d9a", "b0ded8ae-028d-5861-b051-b97b2bedacee=a35c2cef65161e99c10b99e533a418a63fc9e62195fa221147d154ee26dc89c8", "b401a76b-430d-5293-b035-0706a9ac9e03=d2b71a0029ea9905a6a31636bc5018e1e9adc452b77f40be5e87c8dc75dd4c13", "bd10f9c8-b51f-537d-adf9-cef57d23c8e9=30b53cd3a67993c8863973b752cdd8dfba8ae85c090ebebb2cda032a2038fb77", "ed82e2bd-4a0d-56ad-91b0-e200b1d565ed=4aefece0c7c3140826ed971fc4bb17fa61d8dee4b24674ba0225ee3224c1c93d", "edf944ac-8262-5137-bb33-e972dae56799=a7da996410377f65b88b43d652dda9dd4fa8509173f8835e0139fd7844ac051e", "f085026b-b5b9-5d17-a289-0d9179050ae5=51c47e26404639ac5f16faacc0449b9906e020795f58f9b20fe08436b5bb4efc"]
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
