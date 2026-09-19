---
id: 7c3e9f12-4a5b-4d6e-8f1a-2b3c4d5e6f70
title: "A→C loop: usage instrumentation, pack assembly, weak-seed demotion"
type: knowledge
status: active
created_at: "2026-09-15T21:30:00+00:00"
updated_at: "2026-09-19T18:38:57.463846542+00:00"
references: ["9f2a1c3e-7b44-4a5f-9c6d-2e8f1a4b7c90"]
category: decisions
tags: ["decision", "usage-tracking", "context-pack", "graph-routing", "benchmark"]
verified_against: ["9f2a1c3e-7b44-4a5f-9c6d-2e8f1a4b7c90=d0b459c4fddebb23d90b51e258db9a18fbf434099c80704603b31e213fcf0b21"]
---

# A→C improvement loop results (2026-09-15)

Three self-contained iterations, each measured against a frozen corpus
(`--status all`, because source edits legitimately re-flag referencing
knowledge as stale — a benchmark-hygiene requirement: never compare runs
across a corpus boundary).

## Iter 1 — usage instrumentation (schema v4)

`deliveries` + `entity_usage` tables. `capture-event` is one process per
event, so the DB is the delivery state machine: pack/search deliveries
record entity rows as `pending`, `post_tool_use` marks `hit`, the next
delivery or `session_end` flushes the rest to `miss`. Doctor reports hit
rate, dead weight, never-retrieved files. Gotcha found live: `cogz_dir`
is relative in capture context, so absolute hook paths needed canonical
normalization before prefix-stripping.

## Iter 2 — pack dedup (verified counterfactually)

Sections deduplicate on Jaccard≥0.5 over content line-sets, applied to
FULL excerpts before any compression. Verified by replaying dedup over
stored packs (identical inputs): dup_max 0.548→0.193, dup packs 41→0,
zero expected entities dropped. Compressing before dedup breaks it —
shrunken excerpts no longer overlap their containers.

## Iter 3 — weak-KNN-seed demotion (the v1 regression fix)

Mechanism, not removal: KNN hits below `graph_seed_min_sim` (default
0.7 cosine) still seed traversal, but candidates reachable ONLY through
sub-floor seeds are marked weak and demoted to the expansion set —
inform, never displace. FTS seeds always direct-eligible.

- Sweep on v1: floor 0.5/0.6 = no-op; ≥0.7 plateaus at MRR 0.639
  (vs 0.599 unfloored), R@20 untouched, neg_ok unchanged.
- On v2 the floor costs ~4 entity-hits of direct recall vs unfloored;
  demotion recovers ~half into expansion. Conditional gating on
  per-query FTS/KNN strength was measured and rejected — the strength
  distributions overlap in the 0.67–0.78 band, so a conditional is
  curve-fitting, not a mechanism.

## Iter 4 — pack assembly

Measured decomposition first: ~37 expected entities were retrieved but
budget-dropped; ~31 never retrieved; drops concentrate on whatever
sorts lowest (obs/functions/rules). Shipped:

- `compress_tail`: sections past rank 20 get minimal excerpts (file 8,
  fn/class 4, knowledge/obs/rule 8 lines). Weak evidence earns a
  signature, not 15 lines.
- Overflow index: budget-dropped entities collapse into a bounded
  "Also relevant" section of `type:title:id` pointers — real
  discoverability instead of invisible drops.
- Post-relax demotion: `relax_code_sections` regrows excerpts into
  fresh overlaps; regrown dupes are demoted to 4-line pointers (entity
  stays present, content delivered via the container).

## Numbers (frozen corpus, status=all)

| metric | before loop | after |
|---|---|---|
| v1 MRR / R@20 | 0.599 / 0.929 | 0.639 / 0.929 |
| v1 pack recall | 0.651 | **0.963** |
| v2 MRR / R@20 | 0.388 / 0.680 | 0.415 / 0.651 |
| v2 pack recall | 0.482 | **0.716** |
| v2 dup_max / packs_w_dup | 0.259 / 6 | 0.252 / 3 |
| neg_ok | 0.75 / 0.5 | 0.75 / 0.5 |

vs the original pre-loop baselines (older corpus): v2 MRR 0.367→0.415,
pack dup_max 0.548→0.25, dup packs 41→3.

## Iter 4b — pool width + unified overflow index

The residual was wrong: misses were not just budget — entities ranked
below the pack's `task_max_results` pool cutoff never entered assembly
at all. Swept the cutoff on the rebuilt corpus:

| task_max_results | pack recall | packs_w_dup | exp_trunc |
|---|---|---|---|
| 25 | 0.711 | 3 | 23 |
| 40 | **0.831** | 8 → **0** | 33 |
| 60 | 0.831 | 21 | 41 |

Knee at 40: +0.12 recall, no gain beyond. Promoted to the default.

The dup cost of a wider pool was absorbed by generalizing the overflow
index into the union of ALL dropped entities — dedup losers, budget
drops, and regrown dupes whose minimal excerpt still overlaps get
pointer lines (type:title:id), sorted by relevance. Content is
delivered once per pack; presence never dies. Result: zero packs with
duplicate content at recall 0.831 (v2) / 1.0 (v1).

## Iter 4c — dogfooding find: index exit deadlock

`cogz index` hung at exit on every run: `run_index` bound
`let conn = storage.conn()` at function scope, then `update_baseline`
re-acquired `storage.conn()` → non-reentrant mutex self-deadlock.
strace showed main thread futex-waiting on a stack address 0.5ms after
`peel_to_commit` read HEAD. All work had committed — only process exit
was lost, which is why it went unnoticed. Fixed by scoping the guard.
Pattern to watch: `let conn = storage.conn()` held while later calls
take `&storage`.

## Residual

- exp_trunc 33 (v2): expected entities delivered as stubs. This is the
  fixed 8K budget's presence-vs-depth zero-sum; raising the budget is
  an agent-context cost that usage hit-rate data should arbitrate —
  not another benchmark knob.
- v2 neg_ok 0.5 — real semantic collisions (postgresql-pool → `open`
  at 0.87), not signal noise. Needs content judgment, not routing.
