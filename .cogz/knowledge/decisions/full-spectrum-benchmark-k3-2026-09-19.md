---
id: c4d5e6f7-8a9b-4c0d-9e1f-2a3b4c5d6e7f
title: "Full-spectrum benchmark k3: verify cue adoption measured, retrieval/pack/degradation baselines"
type: knowledge
status: active
created_at: "2026-09-19T21:00:00Z"
updated_at: "2026-09-21T19:22:52.888442535+00:00"
references: ["e7c1a2b3-4d5e-4f6a-8b9c-0d1e2f3a4b5c", "f2a3b4c5-6d7e-4f8a-9b0c-1d2e3f4a5b6c", "5fa33147-970e-5b48-96a5-a8daef233491", "dabbf2a6-d29f-4772-a2df-6d264b4b5461"]
category: decisions
tags: ["ab-eval", "benchmark", "verify", "retrieval", "context-packs"]
verified_against: ["5fa33147-970e-5b48-96a5-a8daef233491=2661b279852a95636ed24a7d5891493aa98ee4d04c493caf38ba288303763fc5", "dabbf2a6-d29f-4772-a2df-6d264b4b5461=be1e606b7569de03e4447e071beaef13d94a5cd145d10aca14fde3a18ace1488", "e7c1a2b3-4d5e-4f6a-8b9c-0d1e2f3a4b5c=9391864e03a9d8ad59b153b5bfa919559c07225dba195223022dba5cdddb0344", "f2a3b4c5-6d7e-4f8a-9b0c-1d2e3f4a5b6c=c6efeb27cc7705ce0a90cec6d535de3b8fffbaf8a1fa495e0b083fe48f141629"]
---

# Full-spectrum benchmark (2026-09-19): what the current version can do

Decision-quality run across retrieval, packs, degradation, telemetry, and
agentic replay. Corpus: CogZ self-repo (1,933 entities) + 14 seeded
target-repo worktrees + 4-task k3 agent replay.

## Retrieval (78-query queries_v2, self-repo)

| arm | P@5 | MRR | R@20 | neg_ok | latency |
|---|---|---|---|---|---|
| hybrid | 0.103 | 0.336 | 0.659 | 0.6 | ~0.8s steady (3.7s under embed load) |
| fts-only | 0.097 | 0.204 | 0.634 | 0.0 | ~0.6s |
| no-expand | 0.100 | 0.336 | 0.506 | 0.6 | ~0.8s |

- **Graph expansion is a pure recall lever**: +0.15 R@20 at identical MRR.
- **FTS-only degradation is graceful but real**: MRR −39%, silence gate fully
  absent (neg_ok 0.0 — no semantic signal to gate on), 6× faster.
- Per-intent: code MRR 0.44, graph 0.57 (best), knowledge 0.19, compliance
  0.17 (weakest — the code-drowning pattern again).

## Seeded-knowledge retrieval (14 worktrees × 15 queries)

R@20 **0.892** but MRR **0.202** / P@5 **0.190**. Expected seeds sit at rank
~5 reliably — code entities dominate ranks 1-4. Recall is strong; rank
precision is weak. If consumers only read top-3, knowledge loses.

## Context packs

- Mean: ~8.2K tokens, 75.6 sections, **87% code entities** (60 functions +
  5 classes + 4 files), 11% knowledge-type.
- Orientation recall 0.857 at the 8192 default — **and identical at 4096**.
  1024→0.70, 2048→0.74. The default budget spends ~2× the tokens needed.
- Truth-file section coverage: 3.1% — orientation comes from breadth, not
  precision.

## Silence gate (T=0.05)

6/10 negatives silenced. The 4 leaks are programming-domain-adjacent queries
(elasticsearch, postgres pooling, react lifecycle, terraform) that escape
via `knowledge_strength` 0.645–0.665 — **fully overlapping** the positive
range (0.639–0.728). No threshold separates them; needs a different signal
(joint code+knowledge requirement, top-1 distance cap, or gradient).

## k3 agentic replay — the verify-loop measurement

4 tasks, write-time drift notice live. Export inspection confirms the
notice reached **all 4 agents' context** ("N knowledge entities reference
code in <path> and drifted since last verified" appended to scoped packs).

| task | saves | drift created | verified | final drift |
|---|---|---|---|---|
| 840a4a2 | 6 | 4 | 4 | **0 — loop closed** |
| cb6f5f4 | 20 | 6 | 2 | 4 — partial |
| 0be1c8c | 13 | 7 | 0 | 7 — ignored |
| c1f7865 | 12 | 9 | 0 | 9 — ignored |

**2/4 agents closed or partially closed the loop; 2/4 ignored the cue.**
vs k2's 0/14 — the write-time surface produces real adoption, unevenly.

## FAIL_TO_PASS k3 (0be1c8c excluded — env artifact in all arms)

- 840a4a2: 122/0 (=k, =k2)
- c1f7865: **206/0 — first arm ever to fully pass** (k/k2: 205/1)
- cb6f5f4: 125/8 — **identical to k2's steering failure**

The cb6f5f4 replication is the sobering result: two independent agents, same
seeds, same wrong 8 tests. Verify adoption didn't prevent it — `verify`
checks *provenance freshness*, not *semantic agreement with hidden truth*.
A verified-but-divergent seed is still a confident wrong prior.

## Timing (4 shared tasks, mean)

k 987s · k2 1071s · k3 1165s. Verify work adds steps; n=4 too small to
separate.

## Verdict inputs

- Working: write-time cue (delivery 4/4, adoption 2/4), seeded recall 0.89,
  expansion recall +15pts, FTS degradation bounded, budget headroom 2×.
- Open: rank-precision on knowledge queries, in-domain negative leakage,
  seed-vs-truth correctness (replicated, not noise), uneven verify adoption,
  write-back still ~3 obs/14 tasks.
