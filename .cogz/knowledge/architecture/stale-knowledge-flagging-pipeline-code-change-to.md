---
id: 1d847b1e-6fd4-491d-86eb-bdbcdb7be8c7
title: Stale-knowledge flagging pipeline — code change to frontmatter status
type: knowledge
status: stale
created_at: "2026-09-13T21:42:35.374039918+00:00"
updated_at: "2026-09-15T16:50:45.769953054+00:00"
references: ["eeeb377b-1fab-5129-b252-f88d01e403c7", "8c379ddd-7139-527a-8c34-5ed0539742ca", "90ee1373-02db-5bd1-8fb4-90243806dbc7"]
category: architecture
tags: ["architecture", "stale-flagging", "lifecycle", "file-first"]
---

# Stale-knowledge flagging pipeline

When code entities change (reindex, file_save hook), `flag_stale_knowledge` marks knowledge-layer entities that reference them as `status: stale`.

Flow:
1. `get_edges_involving_batch(changed_code_ids)` → find sources with `references` OR `auto_references` edges pointing at changed code. Both edge types covered — auto-linked knowledge flags too.
2. Batch-fetch referencing entities, filter to type in {observation, rule, knowledge} AND status='active'.
3. Acquire `file_lock()` for the whole RMW batch.
4. Per entity: read canonical file → set frontmatter `status: stale` + `updated_at` → write file → validate via `transition_status` (illegal transition → revert file write) → `sync_single_file` re-derives DB row (status, hash, edges) from the file.
5. One `code_changed` event records all affected IDs.

Non-obvious constraints:
- File-first is literal: the frontmatter is flipped on disk BEFORE the DB is touched, and the DB row is re-derived from the file — not updated directly. This is what keeps file and DB from drifting.
- Deleted-file stale entities are transitional: `cogz reset` + `cogz index` does NOT recreate them — no canonical file exists. Surviving files' `references` frontmatter is the historical record; graph expansion skips dangling refs.
- Failed-file protection: files that fail to read/parse are excluded from stale-marking input — a parse error is not a deletion.
- Review workflow gap: entities flagged stale accumulate; `stale → active` requires manual review and frontmatter edit. No `doctor --review-stale` yet (backlog item 2).