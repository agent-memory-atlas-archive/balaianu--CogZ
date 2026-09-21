---
id: c8d4e2b3-af5e-4b9c-9d6e-2f3e4b5c6d7e
title: "Hybrid delivery validated: pack-gate removal + full semantic arm (replay run5)"
type: knowledge
status: active
created_at: "2026-09-18T00:00:00Z"
updated_at: "2026-09-21T19:22:53.281956736+00:00"
references: ["b7c3f1a2-9e4d-4a8b-8c5d-1f2e3a4b5c6d"]
category: decisions
tags: ["decision", "ab-eval", "context-packs", "hybrid-search", "silence-gate", "delivery-model"]
verified_against: ["b7c3f1a2-9e4d-4a8b-8c5d-1f2e3a4b5c6d=0e9d5ea7d99fce4f117314f2ab8e9f3ef795cfb0d514f5fcb3c59b458a3d7863"]
---

# Hybrid delivery validated — supersedes the pack-gate recommendation

Follow-up to `agent-replay-layered-eval-findings-2026-09-17`. The earlier decision
recommended gating Tier-1 task push on retrieval confidence. Calibration
against the benchmark corpus proved that gate structurally wrong; it was
removed and replaced by a clean split: packs ship retrieval output,
agent-facing search keeps the silence gate. The full semantic product was
then measured as a fourth arm (hyb) on the same 14 tasks.

## Why the pack gate was removed (not retuned)

- Deployed hooks ran `--fts-only`, where the gate can never fire — the
  measured-winning configuration was already ungated push.
- Signal calibration on the real corpus: 14 real task prompts scored
  code_strength 0.238-0.485 vs the 0.64 floor — every one would be
  silenced. Nonsense queries scored 0.170-0.240. **The distributions
  overlap**; no threshold separates them.
- Identifier queries (`ScheduledTaskStore`) can't lexically match —
  FTS5 has no snake_case splitting/stemming — so the vector channel is
  the only path, and the gate killed exactly the queries where
  semantics matter most.
- Pack demotion ≈ deletion: pull usage is 2-4 calls/run, so gated
  content is effectively lost.

Changes: `tier1_gated` removed from assemble; `PackMetadata.gated`
replaced by `signals` (persisted on prompt_submit events + MCP
get_context — provenance for whether semantics ran); new
`SearchParams.silence_gate` (true for CLI/MCP search, false from
`context/query_sections`) — packs bypass wholesale silencing but keep
the per-result `min_relevance` floor.

## run5 results — hyb (full semantic) vs prior arms

All 14 runs: `search_mode=hybrid`, signals present, packs ~8.1k tokens.

- **hyb vs bare**: −31.9% median time, p=0.039, 8/9 wins. Completion
  hyb-only=3, bare-only=0. Recall 0.9 vs 0.8.
- **hyb vs thin**: completion hyb-only=2, thin-only=0; time a coin
  flip (thin faster on easy tasks). Same shape as cogz vs thin.
- **hyb vs cogz (semantic vs FTS push)**: −16.7% median tokens,
  steps −7.7%, recall +3/0, completion +1 (539f8ed). The semantic pack
  is *cheaper and more precise* — full-prompt embedding filtered the
  boilerplate noise that polluted FTS task queries.
- **FAIL_TO_PASS**: mostly arm-neutral. cb6f5f4 hyb clean 133/0 (beats
  cogz's 131/2). One regression: 05aa61d hyb 43/15 vs cogz 56/2 — hyb
  finished faster but left more failing tests. Mixed, within noise.

## Decision

**Hybrid is the default delivery path.** It beats bare decisively and
dominates the FTS arm on token cost and file-selection quality. Keep
`--fts-only` hooks as the degradation path (no models → FTS packs),
not as a performance optimization — the measured FTS advantage was an
artifact of comparing ungated-noise vs gated-silence, not a real win.

## Harness lesson learned this round

`capture-event` spawns background reindex; pointing it at a `.cogz`
copy without source files mass-stales every entity and looks exactly
like broken vector tables. Diagnosis cost ~an hour. Rule: probe DBs
only via read-only commands (`search`, `context`), never `capture-event`,
unless the copy sits in a real worktree.
