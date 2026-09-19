---
id: dabbf2a6-d29f-4772-a2df-6d264b4b5461
title: "Drift-repair loop does not close: zero verify_knowledge calls across 14 agent runs"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-19T09:37:49.727263830+00:00"
references: ["00f608f0-2ce9-567d-afb3-f6418f62079e"]
verified_against: ["00f608f0-2ce9-567d-afb3-f6418f62079e=fcd51723a581b3ae2777ac405474cc292ac374ffdc4c8deebda2923e22b7f830"]
---

Across the ab3 k-arm, agents' edits produced 50 drifted knowledge entities
in 13 worktrees — and `verify_knowledge` was called **zero times**, despite
drift markers (`⚠ drift: N ref(s) changed since verified`) appearing in
packs and search results.

The mechanism works; the affordance doesn't. Agents saw the marker but had
no instruction connecting "this entity drifted" to "run verify_knowledge /
`cogz verify` to re-stamp it". Demoted entities stayed demoted for the rest
of the run — correct behavior, but the loop is open.

To close it, the surfacing needs to be actionable, not just informative:
the pack marker could name the verify command, or the agent prompt could
instruct verification of drifted entities it relied on. Until then, drift
is a one-way ratchet in any session that edits referenced code.
