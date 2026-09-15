---
id: b64c3858-c1ca-43c6-82f3-7ffb63404264
title: "Source-type balancing in hybrid search — proportions, normalization, quota semantics"
type: knowledge
status: stale
created_at: "2026-09-13T21:43:07.489014965+00:00"
updated_at: "2026-09-15T16:50:47.183915816+00:00"
references: ["94ab058f-a75a-508d-b763-1bc4ff58df41", "00b52191-18b2-5b98-8bf3-3bf155ba3ee2", "f13ff191-bfac-5eaa-bee1-bde497d80fe1"]
category: architecture
tags: ["architecture", "search", "rrf", "ranking"]
---

# Source-type balancing in hybrid search

Problem it solves: code entities vastly outnumber knowledge entities, and text-dense knowledge entries can dominate FTS ranking for code questions. The fix splits every channel by entity type and merges with a detected proportion.

**Mechanics** (`src/search/hybrid.rs`, `src/search/balance.rs`):
- FTS results split into code/knowledge ID lists; KNN runs per embedding space (code model, knowledge model).
- `detect_proportions` computes code_prop/knowledge_prop from KNN distances AND FTS match *rates* (matches/collection_size — raw counts would bias toward code). `min_source_proportion` floors each side.
- Each type's lists are RRF-fused separately with the configured weights — proportions are deliberately NOT applied to RRF weights, so uneven weights don't counteract the balance.
- Then each fused list is max-normalized to [0,1] and scaled by its proportion before interleaving.

**Consequence to know:** normalize-then-scale makes the proportion a *ceiling*, not a weight — the top knowledge hit scores exactly knowledge_prop and can never outrank the top code hit when code_prop > knowledge_prop. Representation is guaranteed; cross-type merit ordering is not. Under evaluation for its effect on P@5 (observation from 2026-09-14, backlog item 28).