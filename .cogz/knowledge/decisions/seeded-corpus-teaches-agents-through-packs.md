---
id: 8f2c1a9e-3b7d-4e5f-9a2c-6d1e8f0b4a52
title: Seeded entries teach agents through the pack, not per-harness skill files
type: knowledge
status: active
created_at: "2026-10-02T00:00:00Z"
updated_at: "2026-10-02T00:00:00Z"
references: []
category: decisions
tags: ["seeds", "onboarding", "harness-agnostic", "design"]
---

`cogz init` seeds three corpus entries (ingestion protocol, authorship contract,
memory mechanics) instead of shipping per-agent skill files. Rationale: skills are
harness-scoped — a `.devin/skills/` file never reaches Claude Code or Cursor — but
seeded rules land in the first `session_start` cold-start pack on every harness.
The memory teaches the agent how to use the memory.

Two non-obvious choices:

- **Deterministic UUIDv5 ids** (`SEED_NAMESPACE` + seed name) so a fresh init on
  any machine produces identical entities — dedup across clones stays clean and
  seed ids are stable enough to reference.
- **The young-corpus nudge says "ask the user first"** rather than auto-ingesting:
  committed-corpora make bulk writes team-visible git history, and a session's real
  task should win over a self-improvement detour. The nudge is corpus-state-driven
  (<10 active knowledge/rules), not cadence-driven — it stops firing on its own.
