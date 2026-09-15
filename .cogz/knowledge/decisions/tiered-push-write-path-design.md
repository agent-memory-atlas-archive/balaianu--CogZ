---
id: 7c3d9e21-4f2a-4b8c-9d5e-6f7a8b9c0d1e
type: knowledge
title: "Delivery layer design: push the map, pull the territory"
status: active
references: []
category: decisions
tags: ["decision", "context-packs", "push-pull", "write-path", "backlog-42-44"]
created_at: 2026-09-16T01:10:00Z
updated_at: 2026-09-16T01:10:00Z
---

Decision recorded after the A→C measurement loop (2026-09-15/16):
retrieval hit its measured frontier; the unmeasured axes are delivery
efficacy (pack hit rate ~13%) and write-path salience.

## Push/pull model — "push the map, pull the territory"

- **Tier 0 (always):** identity + rules (~500 tokens). Baseline
  orientation; never gated.
- **Tier 1 (confidence-gated):** search-driven sections ship only when
  channel confidence clears a floor — reuse the silence-gate/strength
  signals already computed. Low-confidence prompt → orientation only.
- **Tier 2 (always):** the overflow pointer index — presence without
  depth; the pull affordance.
- **Structural floor:** tiers 0+2 always ship regardless of hit-rate
  feedback. A learned gate that can shrink everything can death-spiral
  (weak pack → shrink → fewer hits → shrink). The gate adjusts Tier-1
  width only.
- Pull must be cheap before push shrinks — land the graph tools
  (backlog item 13) first or alongside.

## Write-path model — the agent's LLM is the distiller

Mem0's value lives in LLM-distilled writes. CogZ's equivalent without
an internal LLM: the runtime mines candidates from `events`,
`entity_usage`, and `deliveries` (edits after zero-hit searches,
delivered-and-used entities, repeated hot-file edits, error→fix
sequences); a `suggest_observations` MCP tool returns them as
structured candidates; the consuming agent's own model judges and
confirms via `record_observation`. Optional `draft` status isolates
mining noise from packs until confirmed.

## Sequencing

Backlog items 42 (tiered push), 43 (write-path mining), 44 (learned
gate — blocked on data accumulation, weeks of signal needed).
Then the instrument arbitrates: hit-rate per tier/section-type decides
what push earns.
