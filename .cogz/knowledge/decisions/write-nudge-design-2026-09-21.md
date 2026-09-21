---
id: ec757e2c-a5a6-4197-acd6-79ba07a49983
title: "Write-back adoption via pushed nudges, not background extraction"
type: knowledge
status: active
created_at: "2026-09-21T09:39:04Z"
updated_at: "2026-09-21T13:53:58.686844369+00:00"
references: []
category: decisions
tags: ["decision", "write-path", "mining", "nudges", "adoption"]
---

Decision record for fixing organic write-back adoption (backlog 51):
why the write path moved from pull-only `suggest_observations` to
pushed, drafted, deduplicated nudges — and what was deliberately
rejected.

## Rejected alternatives

- **Background LLM extraction** (Mem0/Zep pattern): hidden inference
  cost, non-determinism, violates local-first economics. Claude Code's
  per-turn variant doubles token consumption — the bounded session_end
  variant was still rejected on cost + sparseness grounds.
- **Human review gate** (Devin knowledge pattern): reintroduces the
  friction that caused ~0 organic writes. Review is post-hoc via files
  + `doctor`, not pre-hoc.
- **Session-boundary-only triggers**: mid-session learnings die in
  compaction; too sparse.
- **Auto-written `suggested` entities**: needs state-machine surgery
  and skeletons-as-active would pollute the corpus. Deferred — ignored
  nudges stay discoverable via `suggest_observations`.

## As built

- **Signals**: the four existing mining passes (uncharted_edit,
  recurring_use, hot_file, error_fix) plus `search_miss` — a
  zero-result `search_performed` event followed by indexed file saves
  before the next prompt/session boundary. The gap is demonstrated,
  not speculative.
- **New events**: `search_performed` (query + returned count — doubles
  as the query log the system never had) and `write_nudge_shown`
  (impression ledger: surface + candidate fingerprints).
- **Dedup**: `Suggestion::fingerprint()` keys on the stable anchor
  (file_path / entity_id / delivery_id / error_event_id /
  search_event_id), not volatile evidence — a hot file's growing save
  count does not mint a new nudge. Each fingerprint suppresses for 24h.
- **Surfaces**: file_save/session_start/prompt_submit/post_tool_use/
  session_end hook `additionalContext` (markdown) and search /
  get_context / verify_knowledge MCP responses (`write_nudge` JSON
  block). One builder (`hooks/nudge.rs`), no auto-writes.
- **Payload change**: file_save events now carry `file_path_rel`
  (repo-relative) alongside the raw path — mining matches
  `entities.file_path` and cannot normalize later (no cogz_dir).

## Rationale

The agent's own inference is the extraction engine — already running,
already paid for, holding full context a post-hoc extractor can only
reconstruct worse. The system detects the moment (free heuristics on
the event stream) and ships a drafted candidate; the agent confirms
with one `create_entity` call. Friction drops from
notice-compose-phrase-call to confirm-or-dismiss.

## Measured check (k4 arm, replay run9, 2026-09-21)

8 tasks, same seeded clones as k3 (which produced 0 agent writes on
its 4 shared tasks; k2 historic ~3 observations / 14 tasks):

- **51 impressions** across 8 tasks (0 on one task with no mined
  signals — silence when nothing fires works). 123 unique candidate
  fingerprints; dedup held — no fingerprint re-nudged.
- **2 agent writes** (25% of tasks, 4% of impressions): both genuine
  env-drift gotchas (uncommitted uv.lock → dep resolution; acp/mcp
  version renames), one as `knowledge/gotchas/`, one as a date-
  partitioned observation. Both landed inside a post-impression
  window.
- **All delivery came via hook `additionalContext`** (post_tool_use,
  file_save). MCP write_nudge blocks were never seen — agents made
  zero `search`/`get_context`/`suggest_observations` MCP calls; the
  only 2 cogz MCP calls in 8 tasks were the 2 `create_entity` writes.
- **search_miss never fired**: zero `search_performed` events —
  agents use the `cogz search` CLI, which didn't log the event.
  Fixed post-measurement (cli.rs now records it); the strongest
  designed signal was never exercised in this arm.
- **Impressions dominated by recurring_use + error_fix.** The two
  writes were novel content, not draft confirmations — the nudge
  worked as a persistent write-path reminder more than as
  confirm-the-draft.

Verdict: the delivery machinery works (detect → draft → inject →
dedup), conversion is real but modest. k4 writes/task (0.25) ≈ k2's
organic rate (0.21) — the honest claim is "nudges convert when a real
discovery happens," not "nudges multiply writes." Next lever if
adoption stays the goal: signal quality (search_miss via CLI
telemetry now unblocked; error_fix precision) over louder nudging.

## Second measurement (k5, same 8 tasks, post-fix binary)

k4 exposed four real defects, all fixed and re-measured:

- **`snippet()` panicked on multi-byte chars** — a ✅ at byte 120 of
  tool output crashed `mine_suggestions`, which killed the whole hook
  process silently: events still recorded, but every notice and
  impression died. This — not the 20-event window — is why 0be1c8c
  logged 99 tool calls with zero impressions in k4.
- **`looks_like_error` was keyword-anywhere** — file contents and
  pytest summaries contain 'error'/'failed' legitimately, so 38 of
  0be1c8c's 'errors' were content noise and real pairs drowned.
  Now: exec trusts the `Exit code:` trailer; other tools match
  keywords only in the leading 200 chars.
- **`error_fix` fingerprinted per error_event_id** — a retry grind
  minted a new impression per attempt. Dedup now anchors per tool
  (`error_fix:exec`) — one nudge per tool per day.
- **auto_link created junk edges** — unique-but-generic names
  (`from`, `run`, `both`, `table`) matched bare prose; a dead `from`
  edge orphaned this very document (`code_orphaned` on a doc with
  zero declared refs). Now: single-segment words need a code-shaped
  context (backticks/`(`/`::`/`.`), identifier-shaped names still
  link on word boundary, stale code entities are no longer link
  targets. Production edge count dropped 636→238; the doc healed.
- **`heal_stale_entities` only read declared refs** — edge-flagged
  entities with empty frontmatter refs could never heal. Now unions
  declared + `auto_references` targets; an empty union heals
  (unsubstantiated flag).
- **CLI is the agent surface**: `cogz search`/`context` now print the
  nudge footer, and CLI search records `search_performed` (unblocks
  `search_miss`). Also added `catch_unwind` around mining in hook
  and CLI paths so a future panic degrades to no-nudge instead of
  killing all notices.

k5 results (8 tasks, fresh worktrees, fixed binary):

- **37 impressions** (vs 51) — fewer but better-targeted; error_fix
  fingerprints collapsed 38→3 via per-tool dedup. **Every task got
  impressions** (k4 had a zero-coverage task).
- **18 searches recorded, 4 misses → 4 search_miss candidates** —
  the starved signal now fires end-to-end via CLI telemetry.
- **3 agent writes** (05aa61d subprocess gotcha; 0a0368d uv.lock
  env-drift — the same discovery k4's writer made, reproduced
  independently; c1f7865 busy-callback pitfall — first nudge-arm
  write carrying `references` + `verified_against`, i.e. the agent
  verified it). 3/8 tasks vs 2/8 k4 vs 0/4 k3 shared subset.
- Impression→write 8% (vs 4%); writes land 20-70 events after the
  impression — nudges persist as reminders rather than triggering
  immediate writes.
- **Task outcomes**: zero timeouts (k4 had 2); total wall 12486s vs
  13345s — timing is noise at this n, but no task blew up.

Recurring_use still dominates impressions (94/103 fingerprints) —
it fires on pack-reliance, which is usage, not discovery. If signal
precision becomes the lever, bumping `RECURRING_HIT_MIN` or weighting
by entity age is the next experiment. The write rate (0.375/task)
remains within organic range — the system's value is converting real
discoveries, not manufacturing them.
