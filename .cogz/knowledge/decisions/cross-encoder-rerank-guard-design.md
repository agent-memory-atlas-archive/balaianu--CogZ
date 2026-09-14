---
id: 0f94c6ed-7c73-4467-9b9a-ebe4d1581f51
title: "Cross-encoder rerank: two measured failure modes drove the anchor and code-slot guards"
type: knowledge
status: active
created_at: "2026-09-14T22:33:39.550018+00:00"
updated_at: "2026-09-14T22:33:39.550018+00:00"
references: []
category: decisions
tags: ["decision", "search", "rerank", "benchmark"]
---

# Cross-encoder rerank guard design

Measured on the v2 corpus (78 queries, self-corpus), unguarded reranking fails two distinct ways:

1. **Prose-domain bias.** Both ms-marco TinyBERT and BAAI/bge-reranker-base systematically demote code entities under prose — 16/23 expected functions demoted, avg +2.0 rank. The bias is not score calibration: bge scored an expected function 0.893 yet still ranked it below three prose entities. Passage-domain cross-encoders carry a ranking prior that prose answers prose queries. Fixes: `rerank_anchor` pins the fused head; `rerank_code = false` holds code entities in their fused slots entirely (they are excluded from scoring, so prose can never displace them). `rerank_code = true` exists for future code-capable rerankers.

2. **Expansion starvation.** Replacing `relevance` with near-binary CE probabilities killed graph-expansion seeds (avg_expanded 17.5 → 7.4) because mid-window seeds decayed below `min_relevance`. Fix: expansion seeds use pre-rerank fused scores; `relevance` displays the CE probability.

Sweep results (P@5 / MRR / code-MRR / ms): baseline 0.106 / 0.367 / 0.429 / 386; unanchored 0.091 / 0.315 / 0.113 / 603; bge 0.100 / 0.310 / 0.181 / 15267 (also ~2.5GB RSS — disqualified on footprint); anchor=3 + code-pin 0.109 / 0.370 / 0.429 / 422.

Honest bottom line: on this code-heavy corpus the TinyBERT stage is roughly neutral (+0.003 MRR) at +9% latency — the guards convert an actively harmful stage into a safe one and keep the deep-lift wins, but the model is the ceiling, not the plumbing. bge-q4 and mxbai-xsmall are registered in `src/embed/registry.rs` for future evaluation.
