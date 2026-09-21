---
id: f2a3b4c5-6d7e-4f8a-9b0c-1d2e3f4a5b6c
title: "Replay k2: write-back adoption cracked, verify loop confirmed broken, seeded knowledge shown steering wrong"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-21T19:22:54.218083081+00:00"
references: ["db987e21-598b-47e0-a677-c72a334158eb", "5fa33147-970e-5b48-96a5-a8daef233491"]
category: decisions
tags: ["ab-eval", "knowledge-seeding", "drift", "verify", "write-back"]
verified_against: ["5fa33147-970e-5b48-96a5-a8daef233491=2661b279852a95636ed24a7d5891493aa98ee4d04c493caf38ba288303763fc5", "db987e21-598b-47e0-a677-c72a334158eb=cdf5da853a212977354671d909ee04625281975ff38210f8e1fce096fa9dc64f"]
---

# k2 arm — same seed, new binary (14-tool MCP + drift cues + silence gate)

Follow-up to `knowledge-seeded-delivery-replay-run6`. Identical seed corpus
(12 entities/worktree, 50 resolved references) and task protocol; the only
variables were the consolidated MCP surface, read-side verify cues, and the
0.05 silence threshold.

## Results

- **Completion**: 13/14 — `539f8ed` timed out at 2400s (historically hardest
  task; the k arm nearly failed it too).
- **Speed replicates, now significant**: −29% vs fix (p=0.039), −21% vs hyb
  (p=0.039), −51.5% vs bare (p=0.039), +7.2% vs k (p=0.27, noise). Recall
  0.9 vs bare's 0.8.
- **FAIL_TO_PASS**: 39 fails vs k=37, fix=39, new=38 — flat again.
- **40 pulls, 91 scoped deliveries, 50 drifted entities, 0 verify calls.**

## Write-back: the artifact and the real number

The event table showed ~12 `*_created` events per worktree — but every one
carries a seed title timestamped in the same millisecond burst as the
seed script's `cogz index` sync. **Sync emits `*_created` events for new
canonical files**, so seed ingestion was indistinguishable from agent
writes in naive counting. Filtering to events after `session_start` gives
the real number: **3 observations across 14 tasks** — write-back adoption
remains unsolved. Lesson: per-agent measurements must be scoped to the
agent window (`created_at > session_start`), not the table.

## Verify loop: diagnosed, then fixed on the wrong axis

`verify_calls=0` wasn't apathy — it was geometry. Zero delivered entities
were drifted at delivery time; drift was created by agents' own subsequent
edits. The read-side cue could never reach the writer. Fixed by adding the
write-time notice (see `two-sided-verify-loop`).

## cb6f5f4: seeded knowledge steered confidently wrong

The task went 133/0 (k arm) → 125/8 (k2). The agent leaned hardest on
seeded knowledge there (16 scoped pushes, 6 pulls) and implemented the
*seeded* busy-notification architecture — while the commit's hidden tests
expected different lifecycle semantics. **Seeded knowledge improved
exploration everywhere else but produced a confident wrong answer when it
diverged from test truth.** Knowledge is a prior, not ground truth —
agents need the verify loop precisely so confident-but-stale priors get
checked against reality.

## Decision

Seeding works for speed and adoption; correctness is the open frontier.
Next lever: the write-side notice gives the loop a chance to close —
whether agents act on it is the next thing to measure.
