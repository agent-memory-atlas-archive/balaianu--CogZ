---
id: e7c1a2b3-4d5e-4f6a-8b9c-0d1e2f3a4b5c
title: "Two-sided verify loop: drift cues on both read (delivery) and write (file_save) axes"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-21T19:22:53.916592340+00:00"
references: ["5fa33147-970e-5b48-96a5-a8daef233491", "16dc9fd4-72f1-563e-bea5-9cc6221592d0", "d7473108-0d5d-5f69-bc1a-b64c614136bd", "af3cbdc2-d293-5dc8-9926-176015a9a339", "db987e21-598b-47e0-a677-c72a334158eb"]
category: decisions
tags: ["drift", "verify", "hooks", "delivery", "ab-eval"]
verified_against: ["16dc9fd4-72f1-563e-bea5-9cc6221592d0=58f15cb994de709cc070b951bacc8ca6a8be0caad3d1debbfc0e9f6bf650e1f1", "5fa33147-970e-5b48-96a5-a8daef233491=2661b279852a95636ed24a7d5891493aa98ee4d04c493caf38ba288303763fc5", "af3cbdc2-d293-5dc8-9926-176015a9a339=18e73516edb3081ad185c1f820487827e03f3d021acb955e8638407005d403e2", "d7473108-0d5d-5f69-bc1a-b64c614136bd=8ebc8e7975fe58e2f5864daec76ccd9f8773bd3d8419d8ac6186db9f22b649f0", "db987e21-598b-47e0-a677-c72a334158eb=cdf5da853a212977354671d909ee04625281975ff38210f8e1fce096fa9dc64f"]
---

# The verify cue fired on the wrong axis

The replay k2 run measured the gap precisely: **50 entities drifted across the
fleet, zero `verify_knowledge` calls — and zero drifted entities were ever
delivered.** The read-side cue (drift footer on packs, `drift` block in MCP
responses) only fires when a *delivered* entity is already drifted. But drift
is created by the agent's own edits *after* delivery — so the cue could never
reach the agent at the moment it mattered. Surfacing drift at query time
informs readers; it cannot close the repair loop because the writer never
hears about what they broke.

# Design: two surfaces, one loop

**Read axis (existing):** entities delivered drifted get demoted +
marked in pack sections, MCP `drift` blocks, and CLI footers — all naming
`cogz verify <id>` / `verify_knowledge`. Message: *verify before relying*.

**Write axis (added):** after `handle_file_save` reindexes and the drift
table is fresh, `drift_notice_for_path` maps the saved path to its code
entities and inverts the drift index via `entities_drifted_on` — knowledge
entities whose declared references point at code that just moved. The notice
names them (title + id, capped at 5, `cogz doctor` for the rest) and lands in
`additionalContext` via `hook_additional_context`, alongside or instead of a
scoped rules pack. Message: *your save invalidated these — verify or fix*.

# Human edits are the reason this surface exists

The file_save hook cannot distinguish agent tool calls from a human editor —
and shouldn't try. A human saving `src/foo.rs` in their IDE produces the same
`PostToolUse`-shaped event, the same reindex, the same drift — and now the
same notice, delivered to whichever agent next sees the hook output. This is
the only surface that catches non-agent edits; query-time cues only reach
agents that happen to pull the drifted entity.

# Semantics worth remembering

- **Persistent, not causal-diffed.** The notice re-fires on every save of a
  file with still-drifted referencers until someone verifies — same
  cue-until-closed contract as the read-side footer. A no-op save still
  reminds; that's deliberate.
- **Orthogonal to scoped packs.** Rules *about* the file (scoped push) vs
  knowledge *invalidated by* the file (drift notice) are different axes —
  both can fire on one save.
- **Saving a `.cogz` knowledge file can itself produce a notice** — when
  other entities declared references to it. Knowledge edits are edits too.
- **Verify stays manual.** The notice never auto-verifies; closing the loop
  is a judgment call ("still accurate?" vs "now outdated?") that the agent —
  or human — must make.

# The verify cascade — found and fixed (k3 run)

Dogfooding exposed a quiescence bug: `verified_against` lives in the
canonical frontmatter, and drift baselines stamped the target's **raw byte
hash** — so every `verify` rewrote the file, changed its hash, and
re-drifted every entity referencing it. The queue could never empty on a
connected knowledge graph: verify → index → verify → index…

Fix: drift now compares/stamps `_semantic_hash` — body + frontmatter minus
bookkeeping keys (`status`, `updated_at`, `verified_against`,
`stale_reason`) — stored in `properties` at sync. Raw `content_hash` still
drives sync-skip. One-time rebaseline: old raw-hash stamps read as
`changed` once, then verify re-stamps semantic hashes and the loop
converges (verified: index after mass-verify produced `drifted: 0`).

Regression test: `bookkeeping_rewrites_do_not_cascade_drift`.

k3 adoption result: notice delivered to 4/4 agents; 2/4 verified
(840a4a2 closed its queue fully; cb6f5f4 partial). The cue works;
universal adoption doesn't follow automatically.
