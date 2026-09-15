---
id: 0f94c6ed-7c73-4467-9b9a-ebe4d1581f51
title: "Cross-encoder rerank: implemented, measured, reverted — no real gain on v2 corpus"
type: knowledge
status: stale
created_at: "2026-09-14T22:33:39.550018+00:00"
updated_at: "2026-09-15T16:50:45.585953229+00:00"
references: []
category: decisions
tags: ["decision", "search", "rerank", "benchmark", "reverted"]
---

# Cross-encoder rerank: measured negative result

Backlog item 34 (cross-encoder rerank of top-N fused directs) was fully implemented — ONNX `RerankModel` trait, model delivery, CLI/MCP/hooks plumbing — then **reverted** because the v2 benchmark (78 queries, self-corpus) showed no real gain. The revert commit is `a32d269`; the implementation is recoverable from history (commits `849aa6a`, `0b28e11`).

What the measurement showed:

1. **Prose-domain bias.** ms-marco TinyBERT and BAAI/bge-reranker-base both systematically demote code entities under prose — 16/23 expected functions demoted, avg +2.0 rank. Not a calibration issue: bge scored an expected function 0.893 yet still ranked it below three prose entities.
2. **Expansion starvation.** Writing CE probabilities into `relevance` killed graph-expansion seeds (avg_expanded 17.5 → 7.4); near-binary scores decay below `min_relevance`. Fix found: seed expansion with pre-rerank fused scores.
3. **Guards worked but couldn't create signal.** `rerank_anchor` (pin fused head) + code-slot pinning (code entities excluded from scoring) restored code MRR to baseline — but the residual prose reordering contributed ~nothing (+0.003 MRR overall).
4. **bge is disqualified on footprint**: ~2.5GB RSS, ~15s/query vs TinyBERT's ~650ms.

Sweep (P@5 / MRR / code-MRR / ms): baseline 0.106 / 0.367 / 0.429 / 386; unguarded TinyBERT 0.091 / 0.315 / 0.113 / 603; bge 0.100 / 0.310 / 0.181 / 15267; anchor=3 + code-pin 0.109 / 0.370 / 0.429 / 422.

Bottom line: reranking can fix cross-channel score comparability in principle, but a prose-domain model has nothing to add on a code-heavy corpus. Revisit only with a reranker trained on code retrieval — bge-q4 and mxbai-xsmall were registered candidates at the time of revert.
