---
id: db987e21-598b-47e0-a677-c72a334158eb
title: "Knowledge-seeded delivery validated: seeded canonical knowledge in benchmark corpora (ab3 run6)"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-19T18:39:01.874764710+00:00"
references: ["c8d4e2b3-af5e-4b9c-9d6e-2f3e4b5c6d7e"]
category: decisions
tags: ["decision", "ab-eval", "knowledge-seeding", "drift", "provenance"]
verified_against: ["c8d4e2b3-af5e-4b9c-9d6e-2f3e4b5c6d7e=57a8e25042fc209d8a24db595f49d2780b6cb6028d22d11a05d0d554a84a6002"]
---

# Knowledge-seeded arm (k) — ab3 run6

Follow-up to `hybrid-delivery-validated-ab3-run5`. Instead of relying only on
code retrieval, each of the 14 worktrees was seeded with 12 canonical entities
(7 knowledge, 3 rules, 2 observations) written by hand against real subsystem
code — busy-prompt-queue, send-now race, /schedule flow, config discovery,
test-workflow rules, bridge-extraction observation. References resolved
per-era (functions moved `bot.py` → `bridge.py` mid-history; file-entity
UUIDs use the full relative path as qualified_name).

## Results (14/14 exit 0 — only arm at 100%)

- **vs hyb**: −34.8% median wall-clock (10/13 wins, p=0.09); steps +35%,
  tokens +33% — faster but busier.
- **vs fix**: −41.5% median wall-clock (10/13 wins, p=0.09); steps/tokens
  ~neutral.
- **vs bare**: −40.5% median wall-clock (9/9 wins, p=0.004); recall 0.9 vs 0.8.
- **vs old cogz arm**: −9.3% median (11/13 wins, p=0.02).
- **Pull adoption**: 38 search/pull calls vs 22 (fix), 14 (hyb), 0 (cogz) —
  seeded knowledge made the tool worth querying. Concentrated where knowledge
  mattered: 0be1c8c=17 pulls, 539f8ed=10.
- **FAIL_TO_PASS**: k=37 fails / fix=39 / new=38 — a wash. Speed gains did
  not translate into test-correctness gains. Hard failures bound all arms
  (539f8ed: 27 fails everywhere; 0be1c8c: universal import error — the test
  demands `BUSY_STILL_QUEUED_TEXT` by exact name and no arm produced it).

## Drift lifecycle fired in the field — first production validation

Agents' edits changed code that seeded knowledge referenced: **50 entities
drifted across 13 worktrees** (exactly the expected ones — queue knowledge
drifted on queue tasks). Post-edit search surfaced them demoted with
`drift_count`, not excluded: R@20 held 0.93–1.0 on the seeded query set.
The full loop — edit → drift → demote → still retrievable — works.

## Open issues this run exposed

- `verify_knowledge` got **zero calls** despite 50 drifted entities and pack
  drift markers. The repair loop does not close on its own; agents need the
  verify path surfaced (prompt cue or stronger marker).
- 6/14 tasks ran with a stale `~/.local/bin/cogz` (schema v5 vs v6 DBs) —
  agents hit "schema mismatch" and abandoned the CLI; one deleted `.cogz/`
  outright. Pull adoption is understated.
- Steps/tokens rose while wall-clock fell — the arm is faster, not cheaper.
- The silence gate still doesn't hold on this corpus (negative query
  returned 40 results).
- n=14, single replication; p≈0.09 on the headline deltas — directional.

## Decision

**Knowledge seeding is a real delivery lever, not just provenance hygiene.**
Seeded entities oriented agents (pack hits), survived as retrieval targets,
and their declared references produced the first true drift signal in the
field. Next experiments: surface `verify_knowledge` to close the drift loop,
and seed API-contract facts (exact symbol names) to test whether knowledge
can fix the correctness ceiling — see 0be1c8c's `BUSY_STILL_QUEUED_TEXT`.
