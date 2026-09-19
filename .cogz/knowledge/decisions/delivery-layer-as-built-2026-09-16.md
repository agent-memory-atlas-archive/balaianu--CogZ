---
id: 9e4b2c10-7a1d-4e5f-8b3a-2c6d9e1f4a7b
title: "Delivery layer as built: what shipped vs the design"
type: knowledge
status: active
created_at: "2026-09-16T14:30:00Z"
updated_at: "2026-09-19T18:44:23.915961134+00:00"
references: ["7c3d9e21-4f2a-4b8c-9d5e-6f7a8b9c0d1e"]
category: decisions
tags: ["decision", "context-packs", "tiered-push", "mining", "backlog-13-42-43-44"]
verified_against: ["7c3d9e21-4f2a-4b8c-9d5e-6f7a8b9c0d1e=32ea223ab5a64c729146ee49afa53a2da0d25778868c68c790f126a09d9311ac"]
---

Implementation record for the delivery layer (backlog 13, 42, 43,
44-scaffold). Where the build diverged from the design record.

## As built

- **Tier-1 gate reuses the silence predicate directly** — `tier1_gated`
  in `context/baseline.rs` calls `search::hybrid::should_silence` with
  the same `silence_threshold`/`silence_strength_floor` config. No new
  floor knob: the measured gate is the floor.
- **Gate fires on task packs only.** Escalation is an explicit ask for
  more; FTS-only packs have no signals and never gate. Gated packs
  demote ALL Tier-1 sections to the pointer index — there is no
  partial demotion (flat signals mean the whole candidate set is weak).
- **Tier-0 exemption is partition-based.** `fit_budget` truncates the
  first overflowing section and saturates the budget — budget drops
  only occur after full saturation, so a saturated pack emits no
  pointer index (zero remaining budget). Tier 2's "always present"
  holds only while ≥ ~15 tokens remain.
- **Cold-start sections are `Baseline`-tiered for delivery tracking
  but still budget-fit as one pool** — the whole cold-start pack IS
  the orientation; partitioning it out of the budget would unbound it.
- **`assemble.rs` split** (was 550+ lines): `baseline.rs` (Tier-0
  builders + gate) and `query_sections.rs` (search→section conversion).
- **No `draft` status.** Suggestions are returned, never persisted —
  nothing to isolate from packs. Schema stayed at the single v5
  `entity_usage.tier` migration.
- **Mining runs four signals** in `storage/mining.rs`:
  `uncharted_edit` (all-miss pack → file saves before next prompt
  boundary), `recurring_use` (≥2 hits), `hot_file` (≥3 saves),
  `error_fix` (post_tool_use error → clean call on same tool within 20
  events). `suggestions_requested` event tracks adoption.
- **Pull tracking:** graph-tool results record `pull` deliveries at
  `full` tier — post_tool_use hits measure pull efficacy on the same
  axis as push.
- **Learned gate:** read-side only — `hit_rate_by_tier`,
  `hit_rate_by_type`, doctor prints both. Adaptive demotion stays
  blocked on data accumulation; minimum-sample guardrail is a doc
  contract on the query API, not code.

## Open measurement questions

- Does the gate fire too often? `silence_threshold` 0.02 / floor 0.64
  were tuned for search-result silencing, not pack gating — watch
  `metadata.gated` frequency and `Tier pointer` hit rates in doctor.
- Do `pointer` deliveries get pulled? `pull`-kind hit rate vs `pack`
  is the signal for whether demotion preserves utility.

## Addendum: hit surfaces on hosts without post_tool_use

First real-DB verification found 0 hits in ~43k delivered entities —
not a metric bug: Devin never dispatches `post_tool_use` (fired once
ever, 2026-09-02), and `record_hits` only ran on that event. Extended
hit detection to surfaces that provably fire:

- **file_save** — a saved file matching a delivered entity's
  `file_path` is direct usage evidence. Runs the same
  `detect_touched_entities` path as post_tool_use.
- **prompt_submit** — runs BEFORE `close_open_deliveries`, so a prompt
  naming a delivered file/entity resolves the hit before the new pack
  boundary marks it miss. `prompt_file_tokens` extracts path-like
  tokens (contains `/` or ends in a source ext); the prompt text also
  goes through the title/UUID match.

Second root cause in the same failure: `~/.local/bin/cogz` was v0.1.1
(schema 3) while the DB had migrated to v5 — every hook call died on
version check, silently swallowed by `--hook-json`. Packs hadn't
injected in weeks. Fixed by installing the dev build; the structural
lesson is the hook pipeline needs end-to-end verification, not just
tests — a stale binary fails invisibly.

Emulated a full lifecycle on the real DB (session_start → prompt_submit
→ file_save → session_end): first non-zero hit rates ever — 4 hits
across baseline/full tiers, doctor breakdowns live.

## Addendum 2: full-history replay + lock-contention fix

Replayed the repo's entire history (151 commits / 19 days, Aug 27–Sep
16) as lifecycle events: prompts = commit subjects, file touches =
commit diffs. 173 pack deliveries, 12,268 resolved entity rows.

**Results:** 11.4% overall hit rate — third independent sample
converging on ~12% (13% live window, 12% week sim, 11.4% full
history). ~41 distinct files per pack ≈ 24% of file-space, yet ~94%
of real work touched delivered files — coverage ~4x over chance.
Recall saturated, precision is the cost: Tier-1 can afford to be
much narrower. No temporal decay (12.0% recent vs 11.3% older) — 3
weeks of a hot young codebase is too short to measure staleness.

**Bug found and fixed by the sim:** `record_event` — the FIRST write
in the lifecycle path — had no busy retry. A reindex holding the
write lock past busy_timeout made the whole capture-event call fail:
no event, no close, no pack. That's function loss, not telemetry
loss. Now `with_busy_retry` guards all lifecycle writes
(record_event, close_open_deliveries, record_delivery,
record_delivered, record_hits) and a failed event record degrades to
`event_id: None` so the pack still ships.
