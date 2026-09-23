---
id: 3f964607-cf8d-4cb4-8c98-8b4bc600c0bd
title: "Jev-style calibrated decision head for CogZ — analysis (parked)"
type: knowledge
status: active
created_at: "2026-09-21T22:50:10Z"
updated_at: "2026-09-21T22:50:10Z"
category: decisions
tags: ["decision", "decision-head", "calibration", "research", "future-work", "backlog-52-55"]
---

External research into TypeSafe Jev (the official hosted "System One"
decision model, typesafe.ai — not third-party gateways) and what a
calibrated decision head would solve for CogZ. Parked, may revisit.

## The pattern

CogZ's real friction is fuzzy micro-decisions faked with thresholds.
Nearly every heuristic gate is a decision surface:

- **Intake / write-floor** — nothing gates whether an observation is
  worth persisting (cf. pi-jev-wiki's `derivable_from_code` check:
  "re-derivable from the repo in <1min → don't file it").
- **Dedup** — similarity thresholds + a cap; a same/different/abstain
  judgment would cut near-dup pairs and stop rejected-claim recurrence.
- **Drift triage** — hash drift flags everything; mechanical-vs-semantic
  judgment was done by hand during the 0.3.0 .cogz scrub.
- **Silence gate** — margin heuristic documented as thin.
- **Nudge gating** — `RECURRING_HIT_MIN` + cooldowns vs "will the agent
  act on this?"
- **Pack admission** — RRF rank cutoffs vs per-candidate "earns a slot".
- **Rejected tombstone** (backlog 53) — "seen-and-thrown-out before?"

Backlog items 52–55 all reduce to: we need calibrated judgment and have
heuristics.

## What Jev would buy

- Calibration zero-shot — we have no labeled data for "good intake
  decision"; a local head needs ~30k labeled questions we don't possess.
- `noul`/abstain as first-class output — fixes silent resolution of
  thin-margin cases.
- ~350ms latency fits hook budgets (3–10s).
- Decomposition pattern: five cheap signal questions beat one verdict
  (phishing study: single verdict lost to Haiku, decomposed → 95.1%).

## Why it doesn't fit runtime

Hosted API violates the local-first invariant — `.cogz` content + code
context cannot egress on write/dedup/pack paths. Optional-cloud creates
a two-tier system where smart behavior requires egress. Accuracy only
ties cheap LLMs in independent tests; the moat is latency + calibration.

## Realistic shape if pursued

Jev as offline **oracle**, not runtime: label our events/impressions
ledger (which nudges got acted on, which drift was mechanical), then
distill into a small ONNX head shipping in-runtime like the embedding
models — degrading to heuristics when absent. Teacher-student, not
vendor-in-the-loop.

Horizon ~0.5.0. Extends improvement-backlog items 52–55.
