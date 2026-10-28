---
id: 0b0a90f1-166c-4397-b553-4b5a1b759dbf
title: "Consolidation pipeline — dedup, contradiction, promotion, merge"
type: knowledge
status: active
created_at: "2026-10-28T10:20:46Z"
updated_at: "2026-10-28T10:20:46Z"
references: ["cf109957-2279-457e-8e1e-fc6166765df7", "d0a7fa58-953f-4704-aa38-248ca39f1be6"]
category: architecture
tags: ["consolidation", "dedup", "contradiction", "promotion", "merge", "nli"]
verified_against: ["cf109957-2279-457e-8e1e-fc6166765df7=4b708e98eca90dab196a7ef0bd64ab98ddf845b1d66f48ad2f60fd5e18fb1ed4", "d0a7fa58-953f-4704-aa38-248ca39f1be6=0f45a50a48d07b2e49520d921e826b7d1b0a2ff383b11008bba816bdc153bf28"]
---

Consolidation runs in four phases, split by when they execute (see
`docs/design/consolidation.md`):

1. **Dedup — synchronous on every insert.** Two checks: title match
   (exact case-insensitive, or fuzzy Levenshtein at
   `title_match_threshold` = 0.85 — works without embeddings) and
   embedding cosine similarity (> `dedup_threshold` = 0.85, sets
   `dedup_flagged`). Dedup flags duplicates, it never merges them.
   Degrades to title-only when the embedding model is absent.

2. **Contradiction detection — synchronous on observation/rule
   insert** when `contradiction_check = true` and the NLI model is
   available. Candidates: same-type entities with cosine >
   `contradiction_cosine_threshold` (0.85) and length ratio <
   `contradiction_length_ratio` (5.0). NLI scores P(entailment),
   P(neutral), P(contradiction); P(contradiction) > 0.70 flags the
   pair with a `contradicts` edge in DB + frontmatter. The new entity
   is still created — flagged, not rejected.

3. **Promotion — deferred** (`cogz consolidate`, `consolidate` MCP
   tool, or `session_end` hook). An observation with ≥
   `promotion_threshold` (3) `supports` edges from active
   observations is promoted to a rule: new rule file with
   `derived_from` pointing at the source, `supporting_ids` listing
   supporters, content copied. The source observation is not mutated
   — promotion is additive, and the `derived_from` edge keeps the
   rule traceable.

4. **Merge — deferred, same triggers.** Candidate pairs: active
   observations with cosine > 0.85, then NLI confirm — the pair
   merges when *either* direction's P(entailment) exceeds
   `dedup_nli_threshold` (0.55). Mutual-entailment was abandoned
   (≤4% recall on spec-style text); the cosine prefilter guards
   against subset-facts. Without NLI, candidates are reported in
   dry-run but never merged — conservative, no merge without
   confirmation. Survivor = earlier-created entity; the later one
   gets `status: superseded` + `superseded_by: <survivor_id>`, its
   edges redirect to the survivor, and a `knowledge_merged` event is
   recorded. The superseded file stays on disk; graph expansion
   follows `superseded_by` to the survivor.

**Knowledge and rules are flagged, never auto-merged** — `cogz
doctor` surfaces near-duplicate pairs for human review, since both
types are human-curated.

All thresholds live in `[consolidation]` in `config.toml`. Merge and
prune are multi-step canonical writes — they take `file_lock()` on
`.cogz/.lock` to serialize across processes (see storage concurrency
entity).
